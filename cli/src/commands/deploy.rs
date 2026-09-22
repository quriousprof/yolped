use std::{
    env,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{bail, Result};

use crate::core::{
    logger,
    models::{
        config::JdConfig,
        deployment::{Deployment, DeploymentType, ServerType},
    },
    registry::{DeploymentStatus, Registry},
    remote_runner,
    runner,
    ssh::SshConnection,
};

pub fn run(down: bool, force_local: bool, rebuild: bool) -> Result<()> {
    let config_path = env::current_dir()?.join("yolped.json");
    let config = JdConfig::load()?;
    let deployment_args = config.deployment_args.clone();
    let server = config.server.clone();

    let deployment = Deployment::new(
        config.name,
        config.file_path,
        String::new(),
        server.clone(),
    )?;

    if force_local {
        return run_local(down, &config_path, &deployment, &deployment_args);
    }

    match server {
        ServerType::Local => run_local(down, &config_path, &deployment, &deployment_args)?,
        ServerType::Remote(ref remote) => {
            logger::info(&format!("Deploying to {}@{}...", remote.user, remote.ip));
            println!();
            let conn = SshConnection::connect(remote)?;
            run_remote(down, rebuild, &config_path, &deployment, &deployment_args, &conn, &remote.remote_dir)?;
        }
    }

    Ok(())
}

fn run_local(
    down: bool,
    config_path: &PathBuf,
    deployment: &Deployment,
    args: &[String],
) -> Result<()> {
    if down {
        runner::stop(deployment)?;
        let mut registry = Registry::load()?;
        registry.update_status(config_path, DeploymentStatus::Stopped);
        registry.save()?;
    } else {
        ensure_built_local(config_path, deployment)?;
        let deploy_result = runner::deploy(deployment, args);
        let status = match &deploy_result {
            Ok(_) => runner::check_status(deployment),
            Err(_) => DeploymentStatus::Errored,
        };
        let mut registry = Registry::load()?;
        registry.mark_deployed(config_path);
        registry.update_status(config_path, status);
        registry.save()?;
        deploy_result?;
    }
    Ok(())
}

fn run_remote(
    down: bool,
    rebuild: bool,
    config_path: &PathBuf,
    deployment: &Deployment,
    args: &[String],
    conn: &SshConnection,
    remote_dir: &str,
) -> Result<()> {
    let remote_dir = &conn.expand_path(remote_dir)?;
    remote_runner::check_docker(conn)?;

    let platform = remote_runner::detect_platform(&conn);
    logger::info(&format!("Remote platform: {}", platform));

    if down {
        remote_runner::stop(deployment, conn, remote_dir)?;
        let mut registry = Registry::load()?;
        registry.update_status(config_path, DeploymentStatus::Stopped);
        registry.save()?;
    } else {
        if rebuild {
            remote_runner::remove_images(deployment, conn, remote_dir)?;
        }
        handle_remote_files(deployment, conn, remote_dir)?;
        ensure_built_remote(config_path, deployment, conn, remote_dir, &platform)?;
        let deploy_result = remote_runner::deploy(deployment, conn, remote_dir, args, &platform);
        let status = if deploy_result.is_ok() {
            DeploymentStatus::Running
        } else {
            DeploymentStatus::Errored
        };
        let mut registry = Registry::load()?;
        registry.mark_deployed(config_path);
        registry.update_status(config_path, status);
        registry.save()?;
        deploy_result?;
    }
    Ok(())
}

/// Upload the deployment file and (for compose) handle env files.
fn handle_remote_files(deployment: &Deployment, conn: &SshConnection, remote_dir: &str) -> Result<()> {
    conn.mkdir_p(remote_dir)?;

    let filename = deployment
        .file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Dockerfile");

    let remote_file = format!("{}/{}", remote_dir, filename);

    // --- Upload compose/Dockerfile ---
    if conn.file_exists(&remote_file) {
        logger::warn(&format!("'{}' already exists on the server.", filename));
        if confirm(&format!("Overwrite '{}'?", filename), false)? {
            conn.upload(&deployment.file_path, &remote_file)?;
            logger::success(&format!("'{}' overwritten.", filename));
        } else {
            logger::info(&format!("Keeping existing '{}'.", filename));
        }
    } else {
        logger::info(&format!("Uploading '{}'...", filename));
        conn.upload(&deployment.file_path, &remote_file)?;
        logger::success(&format!("'{}' uploaded.", filename));
    }

    // --- Handle env files for docker-compose ---
    if matches!(deployment.deployment_type, DeploymentType::DockerCompose) {
        let local_dir = deployment
            .file_path
            .parent()
            .unwrap_or(Path::new("."));

        let env_files = remote_runner::parse_env_files(&deployment.file_path)?;

        for local_env_path in env_files {
            // Path relative to the compose file's directory (used for remote placement)
            let rel = local_env_path
                .strip_prefix(local_dir)
                .unwrap_or(&local_env_path);

            let remote_env = format!("{}/{}", remote_dir, rel.display());

            if conn.file_exists(&remote_env) {
                // Already on server — leave it alone
                continue;
            }

            if local_env_path.exists() {
                logger::warn(&format!(
                    "Found '{}' locally but it is missing on the server.",
                    rel.display()
                ));
                if confirm(&format!("Copy '{}' to server?", rel.display()), true)? {
                    // Ensure the parent directory exists on remote
                    if let Some(parent) = Path::new(&remote_env).parent() {
                        conn.mkdir_p(parent.to_str().unwrap_or(remote_dir))?;
                    }
                    conn.upload(&local_env_path, &remote_env)?;
                    logger::success(&format!("'{}' copied to server.", rel.display()));
                } else {
                    bail!(
                        "Env file '{}' is missing on the server. Create it at '{}' before deploying.",
                        rel.display(),
                        remote_env
                    );
                }
            } else {
                bail!(
                    "Env file '{}' is missing both locally and on the server.\n\
                     Create it on the server at '{}' before deploying.",
                    rel.display(),
                    remote_env
                );
            }
        }
    }

    println!();
    Ok(())
}

fn ensure_built_local(config_path: &PathBuf, deployment: &Deployment) -> Result<()> {
    let is_built = match &deployment.deployment_type {
        DeploymentType::Dockerfile => runner::image_exists(&deployment.name),
        DeploymentType::DockerCompose => Registry::load()?
            .deployments
            .iter()
            .find(|e| e.config_path == *config_path)
            .map(|e| e.last_built_at.is_some())
            .unwrap_or(false),
    };

    if !is_built {
        logger::info("No build found. Running `yolped build` first...");
        println!();
        runner::build(deployment)?;
        let mut registry = Registry::load()?;
        registry.mark_built(config_path);
        registry.save()?;
        println!();
    }

    Ok(())
}

fn ensure_built_remote(
    config_path: &PathBuf,
    deployment: &Deployment,
    conn: &SshConnection,
    remote_dir: &str,
    platform: &str,
) -> Result<()> {
    let filename = deployment
        .file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Dockerfile");
    let remote_file = format!("{}/{}", remote_dir, filename);

    // Always ask Docker directly — the registry cache can be stale if images were removed
    let is_built = match &deployment.deployment_type {
        DeploymentType::Dockerfile => remote_runner::image_exists(deployment, conn),
        DeploymentType::DockerCompose => {
            remote_runner::compose_images_exist(conn, &remote_file, &deployment.name)
        }
    };

    if !is_built {
        logger::info("No image found on remote. Building first...");
        println!();
        remote_runner::build(deployment, conn, remote_dir, platform)?;
        let mut registry = Registry::load()?;
        registry.mark_built(config_path);
        registry.save()?;
        println!();
    }

    Ok(())
}

fn confirm(question: &str, default_yes: bool) -> Result<bool> {
    let hint = if default_yes { "Y/n" } else { "y/N" };
    print!("{} [{}]: ", question, hint);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_lowercase();

    Ok(match trimmed.as_str() {
        "" => default_yes,
        "y" | "yes" => true,
        _ => false,
    })
}
