use std::{collections::HashSet, path::{Path, PathBuf}};

use anyhow::{bail, Context, Result};

use super::{
    logger,
    models::deployment::{Deployment, DeploymentType},
    ssh::SshConnection,
};

/// Verify Docker is installed and the current user can access the daemon.
pub fn check_docker(conn: &SshConnection) -> Result<()> {
    logger::info("Checking Docker on remote...");

    let code = conn.exec_stream("docker --version 2>&1")?;
    if code != 0 {
        bail!(
            "Docker is not installed on the remote server.\n\
             Install Docker first, then rerun `yolped deploy`.\n\
             See: https://docs.docker.com/engine/install/"
        );
    }

    // Verify the user can actually reach the daemon
    let daemon_out = conn.exec_output("docker info 2>&1")?;
    if daemon_out.contains("permission denied") || daemon_out.contains("Got permission denied") {
        bail!(
            "Permission denied connecting to the Docker daemon on the remote server.\n\
             Add your user to the docker group and reconnect:\n\n  \
             sudo usermod -aG docker $(whoami)\n\n\
             Then open a new SSH session and rerun `yolped deploy`."
        );
    }
    if daemon_out.contains("Cannot connect") || daemon_out.contains("Is the docker daemon running") {
        bail!(
            "Cannot connect to the Docker daemon on the remote server.\n\
             Make sure Docker is running:\n\n  \
             sudo systemctl start docker"
        );
    }

    Ok(())
}

/// Parse all `env_file` paths referenced in a docker-compose file.
/// Returns paths relative to the compose file's directory.
pub fn parse_env_files(compose_path: &Path) -> Result<Vec<PathBuf>> {
    let contents = std::fs::read_to_string(compose_path)
        .with_context(|| format!("Failed to read '{}'", compose_path.display()))?;
    let value: serde_yaml::Value = serde_yaml::from_str(&contents)
        .with_context(|| format!("Failed to parse '{}'", compose_path.display()))?;

    let compose_dir = compose_path.parent().unwrap_or(Path::new("."));
    let mut seen = HashSet::new();
    let mut paths = Vec::new();

    let Some(services) = value.get("services").and_then(|s| s.as_mapping()) else {
        return Ok(paths);
    };

    for service in services.values() {
        let env_file = match service.get("env_file") {
            Some(v) => v,
            None => continue,
        };

        let raw: Vec<String> = match env_file {
            serde_yaml::Value::String(s) => vec![s.clone()],
            serde_yaml::Value::Sequence(seq) => seq
                .iter()
                .filter_map(|item| match item {
                    serde_yaml::Value::String(s) => Some(s.clone()),
                    serde_yaml::Value::Mapping(m) => m
                        .get("path")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    _ => None,
                })
                .collect(),
            _ => continue,
        };

        for s in raw {
            let resolved = compose_dir.join(&s);
            // Normalize to avoid duplicates from `./x` vs `x`
            let key = resolved
                .canonicalize()
                .unwrap_or_else(|_| resolved.clone());
            if seen.insert(key) {
                paths.push(resolved);
            }
        }
    }

    Ok(paths)
}

/// Build the deployment image on the remote server.
/// Files must already be uploaded before calling this.
pub fn build(deployment: &Deployment, conn: &SshConnection, remote_dir: &str) -> Result<()> {
    logger::info(&format!("Building '{}' on remote...", deployment.name));

    let filename = deployment
        .file_path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Invalid file name in file_path")?;

    let remote_file = format!("{}/{}", remote_dir, filename);

    println!();
    let cmd = match &deployment.deployment_type {
        DeploymentType::Dockerfile => format!(
            "docker build -f '{}' -t '{}' '{}'",
            remote_file, deployment.name, remote_dir
        ),
        DeploymentType::DockerCompose => format!(
            "docker compose -f '{}' -p '{}' build",
            remote_file, deployment.name
        ),
    };

    let code = conn.exec_stream(&cmd)?;
    if code != 0 {
        bail!("Remote build failed (exit code: {})", code);
    }

    println!();
    logger::success(&format!("'{}' built successfully on remote!", deployment.name));
    Ok(())
}

/// Run the deployment on the remote server.
pub fn deploy(
    deployment: &Deployment,
    conn: &SshConnection,
    remote_dir: &str,
    args: &[String],
) -> Result<()> {
    logger::info(&format!("Deploying '{}' on remote...", deployment.name));

    let filename = deployment
        .file_path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Invalid file name in file_path")?;

    let remote_file = format!("{}/{}", remote_dir, filename);
    let extra = args.join(" ");

    let cmd = match &deployment.deployment_type {
        DeploymentType::Dockerfile => format!(
            "docker rm -f '{name}' 2>/dev/null; docker run -d --name '{name}' {extra} '{name}'",
            name = deployment.name,
            extra = extra,
        ),
        DeploymentType::DockerCompose => format!(
            "docker compose -f '{}' -p '{}' up -d {}",
            remote_file, deployment.name, extra
        ),
    };

    logger::info("Starting containers...");
    println!();
    let code = conn.exec_stream(&cmd)?;
    println!();
    if code != 0 {
        bail!("Remote deploy failed (exit code: {})", code);
    }

    logger::success(&format!("'{}' is running on remote.", deployment.name));
    Ok(())
}

/// Stop the deployment on the remote server.
pub fn stop(deployment: &Deployment, conn: &SshConnection, remote_dir: &str) -> Result<()> {
    logger::info(&format!("Stopping '{}' on remote...", deployment.name));

    let filename = deployment
        .file_path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Invalid file name in file_path")?;

    let remote_file = format!("{}/{}", remote_dir, filename);

    let cmd = match &deployment.deployment_type {
        DeploymentType::Dockerfile => format!("docker stop '{}'", deployment.name),
        DeploymentType::DockerCompose => format!(
            "docker compose -f '{}' -p '{}' down",
            remote_file, deployment.name
        ),
    };

    let code = conn.exec_stream(&cmd)?;
    if code != 0 {
        bail!("Remote stop failed (exit code: {})", code);
    }

    logger::success(&format!("'{}' stopped on remote.", deployment.name));
    Ok(())
}

/// Stream logs from a deployment on the remote server.
pub fn logs(deployment: &Deployment, conn: &SshConnection, remote_dir: &str) -> Result<()> {
    let filename = deployment
        .file_path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Invalid file name in file_path")?;

    let remote_file = format!("{}/{}", remote_dir, filename);

    let cmd = match &deployment.deployment_type {
        DeploymentType::Dockerfile => format!("docker logs -f '{}'", deployment.name),
        DeploymentType::DockerCompose => format!(
            "docker compose -f '{}' -p '{}' logs -f",
            remote_file, deployment.name
        ),
    };

    conn.exec_stream(&cmd)?;
    Ok(())
}

/// Check whether the image exists on the remote server.
pub fn image_exists(deployment: &Deployment, conn: &SshConnection) -> bool {
    conn.image_exists(&deployment.name)
}
