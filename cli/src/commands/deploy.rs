use std::{env, path::PathBuf};

use anyhow::Result;

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

pub fn run(down: bool, force_local: bool) -> Result<()> {
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
            let conn = SshConnection::connect(remote)?;
            run_remote(down, &config_path, &deployment, &deployment_args, &conn, &remote.remote_dir)?;
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
    config_path: &PathBuf,
    deployment: &Deployment,
    args: &[String],
    conn: &SshConnection,
    remote_dir: &str,
) -> Result<()> {
    remote_runner::check_docker(conn)?;

    if down {
        remote_runner::stop(deployment, conn, remote_dir)?;
        let mut registry = Registry::load()?;
        registry.update_status(config_path, DeploymentStatus::Stopped);
        registry.save()?;
    } else {
        ensure_built_remote(config_path, deployment, conn, remote_dir)?;
        let deploy_result = remote_runner::deploy(deployment, conn, remote_dir, args);
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
) -> Result<()> {
    let is_built = match &deployment.deployment_type {
        DeploymentType::Dockerfile => remote_runner::image_exists(deployment, conn),
        DeploymentType::DockerCompose => Registry::load()?
            .deployments
            .iter()
            .find(|e| e.config_path == *config_path)
            .map(|e| e.last_built_at.is_some())
            .unwrap_or(false),
    };

    if !is_built {
        logger::info("No build found on remote. Building first...");
        println!();
        remote_runner::build(deployment, conn, remote_dir)?;
        let mut registry = Registry::load()?;
        registry.mark_built(config_path);
        registry.save()?;
        println!();
    }

    Ok(())
}
