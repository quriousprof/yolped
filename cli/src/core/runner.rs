use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    thread,
};

use anyhow::{Context, Result, bail};

use super::{logger, models::deployment::{Deployment, DeploymentType}, registry::DeploymentStatus};

/// Query live Docker state for a deployment
pub fn check_status(deployment: &Deployment) -> DeploymentStatus {
    match &deployment.deployment_type {
        DeploymentType::Dockerfile => check_docker_status(&deployment.name),
        DeploymentType::DockerCompose => check_compose_status(deployment),
    }
}

fn check_docker_status(name: &str) -> DeploymentStatus {
    let Ok(out) = Command::new("docker")
        .args(["inspect", "--format", "{{.State.Running}} {{.State.ExitCode}}", name])
        .output()
    else {
        return DeploymentStatus::Unknown;
    };

    if !out.status.success() {
        return DeploymentStatus::Unknown;
    }

    let text = String::from_utf8_lossy(&out.stdout);
    match text.trim().splitn(2, ' ').collect::<Vec<_>>().as_slice() {
        ["true", _]    => DeploymentStatus::Running,
        ["false", "0"] => DeploymentStatus::Stopped,
        ["false", _]   => DeploymentStatus::Errored,
        _              => DeploymentStatus::Unknown,
    }
}

fn check_compose_status(deployment: &Deployment) -> DeploymentStatus {
    let Some(file_str) = deployment.file_path.to_str() else {
        return DeploymentStatus::Unknown;
    };

    let Ok(out) = Command::new("docker")
        .args(["compose", "-f", file_str, "-p", &deployment.name, "ps", "--all"])
        .output()
    else {
        return DeploymentStatus::Unknown;
    };

    let text = String::from_utf8_lossy(&out.stdout);
    let mut has_running = false;
    let mut has_errored = false;
    let mut has_any = false;

    for line in text.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() { continue; }
        has_any = true;
        if line.contains(" Up ") || line.contains(" running") {
            has_running = true;
        } else if line.contains("Exited (0)") {
            // stopped cleanly — not an error
        } else if line.contains("Exited") {
            has_errored = true;
        }
    }

    if !has_any       { DeploymentStatus::Unknown }
    else if has_running { DeploymentStatus::Running }
    else if has_errored { DeploymentStatus::Errored }
    else               { DeploymentStatus::Stopped }
}

/// Check whether a Docker image with the given name exists locally
pub fn image_exists(name: &str) -> bool {
    Command::new("docker")
        .args(["image", "inspect", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Execute the build step for a deployment
pub fn build(deployment: &Deployment) -> Result<()> {
    logger::info(&format!("Building '{}'...", deployment.name));

    match &deployment.deployment_type {
        DeploymentType::Dockerfile => build_dockerfile(deployment),
        DeploymentType::DockerCompose => build_compose(deployment),
    }
}

fn build_dockerfile(deployment: &Deployment) -> Result<()> {
    let file_path = &deployment.file_path;

    let context_path = file_path
        .parent()
        .context("Could not determine build context: Dockerfile has no parent directory")?;

    let file_str = file_path
        .to_str()
        .context("Dockerfile path contains invalid UTF-8")?;

    let context_str = context_path
        .to_str()
        .context("Build context path contains invalid UTF-8")?;

    let mut cmd = Command::new("docker");
    cmd.args(["build", "-f", file_str]);

    if !deployment.name.is_empty() {
        cmd.args(["-t", &deployment.name]);
    }

    cmd.arg(context_str)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd
        .spawn()
        .context("Failed to spawn 'docker build'. Is Docker installed and running?")?
        .wait()
        .context("Failed to wait for 'docker build' process")?;

    if !status.success() {
        bail!(
            "Docker build failed (exit code: {})",
            status.code().map_or_else(|| "unknown".to_string(), |c| c.to_string())
        );
    }

    logger::success(&format!("'{}' built successfully!", deployment.name));
    Ok(())
}

/// Build and run the deployment in detached mode
pub fn deploy(deployment: &Deployment, args: &[String]) -> Result<()> {
    logger::info(&format!("Deploying '{}'...", deployment.name));

    match &deployment.deployment_type {
        DeploymentType::Dockerfile => deploy_dockerfile(deployment, args),
        DeploymentType::DockerCompose => deploy_compose(deployment, args),
    }
}

fn deploy_dockerfile(deployment: &Deployment, args: &[String]) -> Result<()> {
    // Remove any existing container (running or stopped) so docker run can reuse the name
    Command::new("docker")
        .args(["rm", "-f", &deployment.name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok();

    let mut cmd = Command::new("docker");
    cmd.args(["run", "-d", "--name", &deployment.name]);
    cmd.args(args);
    cmd.arg(&deployment.name); // image name — must come last
    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());

    let status = cmd
        .spawn()
        .context("Failed to spawn 'docker run'")?
        .wait()
        .context("Failed to wait for 'docker run' process")?;

    if !status.success() {
        bail!(
            "docker run failed (exit code: {})",
            status.code().map_or_else(|| "unknown".to_string(), |c| c.to_string())
        );
    }

    logger::success(&format!("'{}' is running.", deployment.name));
    Ok(())
}

fn deploy_compose(deployment: &Deployment, args: &[String]) -> Result<()> {
    let file_str = deployment
        .file_path
        .to_str()
        .context("Compose file path contains invalid UTF-8")?;

    let mut child = Command::new("docker");
    child.args(["compose", "-f", file_str, "-p", &deployment.name, "up", "-d"]);
    child.args(args);
    let mut child = child
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn 'docker compose up'")?;

    // Read stderr in a thread so the pipe buffer never blocks the child.
    // Capture the output to check for known errors while still printing it live.
    let stderr = child.stderr.take().unwrap();
    let stderr_thread = thread::spawn(move || {
        let reader = BufReader::new(stderr);
        let mut platform_mismatch = false;
        for line in reader.lines().map_while(Result::ok) {
            if line.contains("does not match the detected host platform") {
                platform_mismatch = true;
            }
            eprintln!("{}", line);
        }
        platform_mismatch
    });

    let status = child
        .wait()
        .context("Failed to wait for 'docker compose up' process")?;

    let platform_mismatch = stderr_thread.join().unwrap_or(false);

    if platform_mismatch {
        bail!(
            "Platform mismatch: the image was built for a different architecture than this machine. \
             Add `platform: linux/arm64` (or the correct platform) to your docker-compose service."
        );
    }

    if !status.success() {
        bail!(
            "docker compose up failed (exit code: {})",
            status.code().map_or_else(|| "unknown".to_string(), |c| c.to_string())
        );
    }

    logger::success(&format!("'{}' is running.", deployment.name));
    Ok(())
}

/// Stop and remove the running deployment
pub fn stop(deployment: &Deployment) -> Result<()> {
    logger::info(&format!("Stopping '{}'...", deployment.name));

    match &deployment.deployment_type {
        DeploymentType::Dockerfile => stop_dockerfile(deployment),
        DeploymentType::DockerCompose => stop_compose(deployment),
    }
}

fn stop_dockerfile(deployment: &Deployment) -> Result<()> {
    let status = Command::new("docker")
        .args(["stop", &deployment.name])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to spawn 'docker stop'")?
        .wait()
        .context("Failed to wait for 'docker stop'")?;

    if !status.success() {
        bail!(
            "docker stop failed (exit code: {})",
            status.code().map_or_else(|| "unknown".to_string(), |c| c.to_string())
        );
    }

    logger::success(&format!("'{}' stopped.", deployment.name));
    Ok(())
}

fn stop_compose(deployment: &Deployment) -> Result<()> {
    let file_str = deployment
        .file_path
        .to_str()
        .context("Compose file path contains invalid UTF-8")?;

    let status = Command::new("docker")
        .args(["compose", "-f", file_str, "-p", &deployment.name, "down"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to spawn 'docker compose down'")?
        .wait()
        .context("Failed to wait for 'docker compose down'")?;

    if !status.success() {
        bail!(
            "docker compose down failed (exit code: {})",
            status.code().map_or_else(|| "unknown".to_string(), |c| c.to_string())
        );
    }

    logger::success(&format!("'{}' stopped.", deployment.name));
    Ok(())
}

/// Stream logs for a running deployment
pub fn logs(deployment: &Deployment) -> Result<()> {
    match &deployment.deployment_type {
        DeploymentType::Dockerfile => logs_dockerfile(deployment),
        DeploymentType::DockerCompose => logs_compose(deployment),
    }
}

fn logs_dockerfile(deployment: &Deployment) -> Result<()> {
    Command::new("docker")
        .args(["logs", "-f", &deployment.name])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to spawn 'docker logs'")?
        .wait()
        .context("Failed to wait for 'docker logs'")?;
    Ok(())
}

fn logs_compose(deployment: &Deployment) -> Result<()> {
    let file_str = deployment
        .file_path
        .to_str()
        .context("Compose file path contains invalid UTF-8")?;

    Command::new("docker")
        .args([
            "compose", "-f", file_str, "-p", &deployment.name, "logs", "-f",
        ])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to spawn 'docker compose logs'")?
        .wait()
        .context("Failed to wait for 'docker compose logs'")?;
    Ok(())
}

fn build_compose(deployment: &Deployment) -> Result<()> {
    let file_str = deployment
        .file_path
        .to_str()
        .context("Compose file path contains invalid UTF-8")?;

    let status = Command::new("docker")
        .args(["compose", "-f", file_str, "-p", &deployment.name, "build"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to spawn 'docker compose'. Is Docker installed and running?")?
        .wait()
        .context("Failed to wait for 'docker compose build' process")?;

    if !status.success() {
        bail!(
            "docker compose build failed (exit code: {})",
            status.code().map_or_else(|| "unknown".to_string(), |c| c.to_string())
        );
    }

    logger::success(&format!("'{}' built successfully!", deployment.name));
    Ok(())
}
