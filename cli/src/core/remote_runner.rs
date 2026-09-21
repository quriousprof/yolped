use std::path::Path;

use anyhow::{bail, Context, Result};

use super::{
    logger,
    models::deployment::{Deployment, DeploymentType},
    ssh::SshConnection,
};

/// Verify Docker is installed on the remote server.
pub fn check_docker(conn: &SshConnection) -> Result<()> {
    let code = conn.exec_stream("docker --version > /dev/null 2>&1")?;
    if code != 0 {
        bail!(
            "Docker is not installed on the remote server.\n\
             Install Docker first, then rerun `yolped deploy`.\n\
             See: https://docs.docker.com/engine/install/"
        );
    }
    Ok(())
}

/// Build the deployment image on the remote server.
pub fn build(deployment: &Deployment, conn: &SshConnection, remote_dir: &str) -> Result<()> {
    logger::info(&format!("Building '{}' on remote...", deployment.name));

    let filename = deployment
        .file_path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Invalid file name in file_path")?;

    let remote_file = format!("{}/{}", remote_dir, filename);

    conn.mkdir_p(remote_dir)?;
    conn.upload(&deployment.file_path, &remote_file)
        .with_context(|| format!("Failed to upload '{}'", deployment.file_path.display()))?;

    logger::info(&format!("Uploaded '{}' to remote", filename));

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

    let code = conn.exec_stream(&cmd)?;
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
