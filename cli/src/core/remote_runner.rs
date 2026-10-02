use std::{collections::HashSet, path::{Path, PathBuf}};

use anyhow::{bail, Context, Result};

use super::{
    logger,
    models::{
        config::RegistryConfig,
        deployment::{Deployment, DeploymentType},
    },
    ssh::SshConnection,
};

/// Wrap a string in single quotes and escape any internal single quotes for shell safety.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Build the remote path to the deployment file (e.g. `~/deployments/docker-compose.yml`).
fn remote_file_path(deployment: &Deployment, remote_dir: &str) -> Result<String> {
    let filename = deployment
        .file_path
        .file_name()
        .and_then(|n| n.to_str())
        .context("Invalid file name in file_path")?;
    Ok(format!("{}/{}", remote_dir, filename))
}

/// Detect the remote server's CPU architecture and return the Docker platform string.
pub fn detect_platform(conn: &SshConnection) -> String {
    let arch = conn.exec_output("uname -m").unwrap_or_default();
    match arch.trim() {
        "x86_64"            => "linux/amd64".to_string(),
        "aarch64" | "arm64" => "linux/arm64".to_string(),
        "armv7l"            => "linux/arm/v7".to_string(),
        other               => format!("linux/{}", other),
    }
}

/// Check whether docker-compose images exist for a project on the remote server.
pub fn compose_images_exist(conn: &SshConnection, remote_file: &str, project_name: &str) -> bool {
    conn.exec_output(&format!(
        "docker compose -f {} -p {} images -q 2>/dev/null | head -1",
        shell_escape(remote_file), shell_escape(project_name)
    ))
    .map(|s| !s.trim().is_empty())
    .unwrap_or(false)
}

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
/// Returns paths resolved relative to the compose file's directory.
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

/// Remove all images for the deployment on the remote server.
pub fn remove_images(deployment: &Deployment, conn: &SshConnection, remote_dir: &str) -> Result<()> {
    logger::info("Removing existing images on remote...");

    let remote_file = remote_file_path(deployment, remote_dir)?;

    let cmd = match &deployment.deployment_type {
        DeploymentType::Dockerfile => format!(
            "docker rmi -f {} 2>/dev/null || true",
            shell_escape(&deployment.name)
        ),
        DeploymentType::DockerCompose => format!(
            "docker compose -f {} -p {} down --rmi all 2>/dev/null || true",
            shell_escape(&remote_file), shell_escape(&deployment.name)
        ),
    };

    conn.exec_stream(&cmd)?;
    logger::success("Existing images removed.");
    Ok(())
}

/// Build the deployment image on the remote server.
/// Files must already be uploaded before calling this.
pub fn build(deployment: &Deployment, conn: &SshConnection, remote_dir: &str, platform: &str) -> Result<()> {
    logger::info(&format!("Building '{}' on remote ({})...", deployment.name, platform));

    let remote_file = remote_file_path(deployment, remote_dir)?;

    println!();
    let cmd = match &deployment.deployment_type {
        DeploymentType::Dockerfile => format!(
            "docker build --platform {} -f {} -t {} {}",
            shell_escape(platform), shell_escape(&remote_file),
            shell_escape(&deployment.name), shell_escape(remote_dir)
        ),
        DeploymentType::DockerCompose => format!(
            "docker compose -f {} -p {} build --no-cache",
            shell_escape(&remote_file), shell_escape(&deployment.name)
            // compose reads DOCKER_DEFAULT_PLATFORM env set below
        ),
    };

    let full_cmd = format!("DOCKER_DEFAULT_PLATFORM={} {}", shell_escape(platform), cmd);
    let code = conn.exec_stream(&full_cmd)?;
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
    platform: &str,
) -> Result<()> {
    logger::info(&format!("Deploying '{}' on remote...", deployment.name));

    let remote_file = remote_file_path(deployment, remote_dir)?;
    let extra: String = args.iter().map(|a| shell_escape(a)).collect::<Vec<_>>().join(" ");

    let cmd = match &deployment.deployment_type {
        DeploymentType::Dockerfile => {
            let name = shell_escape(&deployment.name);
            let plat = shell_escape(platform);
            format!(
                "docker rm -f {name} 2>/dev/null; \
                 docker run --platform {plat} -d --name {name} {extra} {name}",
                name = name,
                plat = plat,
                extra = extra,
            )
        }
        DeploymentType::DockerCompose => format!(
            "DOCKER_DEFAULT_PLATFORM={} docker compose -f {} -p {} up -d {}",
            shell_escape(platform), shell_escape(&remote_file),
            shell_escape(&deployment.name), extra
        ),
    };

    logger::info("Starting containers...");
    println!();
    let code = conn.exec_stream(&cmd)?;
    println!();
    if code != 0 {
        // Show container logs so the user can see why it failed
        logger::warn("Deploy failed. Fetching container logs...");
        println!();
        let logs_cmd = match &deployment.deployment_type {
            DeploymentType::Dockerfile => format!(
                "docker logs --tail=50 {} 2>&1",
                shell_escape(&deployment.name)
            ),
            DeploymentType::DockerCompose => format!(
                "docker compose -f {} -p {} logs --tail=50 2>&1",
                shell_escape(&remote_file), shell_escape(&deployment.name)
            ),
        };
        let _ = conn.exec_stream(&logs_cmd);
        println!();
        bail!("Deploy failed — see logs above for details.");
    }

    logger::success(&format!("'{}' is running on remote.", deployment.name));
    Ok(())
}

/// Stop the deployment on the remote server.
pub fn stop(deployment: &Deployment, conn: &SshConnection, remote_dir: &str) -> Result<()> {
    logger::info(&format!("Stopping '{}' on remote...", deployment.name));

    let remote_file = remote_file_path(deployment, remote_dir)?;

    let cmd = match &deployment.deployment_type {
        DeploymentType::Dockerfile => format!(
            "docker stop {}",
            shell_escape(&deployment.name)
        ),
        DeploymentType::DockerCompose => format!(
            "docker compose -f {} -p {} down",
            shell_escape(&remote_file), shell_escape(&deployment.name)
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
    let remote_file = remote_file_path(deployment, remote_dir)?;

    let cmd = match &deployment.deployment_type {
        DeploymentType::Dockerfile => format!(
            "docker logs -f {}",
            shell_escape(&deployment.name)
        ),
        DeploymentType::DockerCompose => format!(
            "docker compose -f {} -p {} logs -f",
            shell_escape(&remote_file), shell_escape(&deployment.name)
        ),
    };

    conn.exec_stream(&cmd)?;
    Ok(())
}

/// Check whether the image exists on the remote server.
pub fn image_exists(deployment: &Deployment, conn: &SshConnection) -> bool {
    conn.image_exists(&deployment.name)
}

/// Pull the deployment image from a registry on the remote server.
///
/// For Dockerfile projects, pulls the image and tags it with the deployment name
/// so the `deploy` step can reference it by name without knowing the registry path.
///
/// For Compose projects, runs `docker compose pull` which pulls all service images
/// referenced in the compose file.
pub fn pull_from_registry(
    deployment: &Deployment,
    conn: &SshConnection,
    remote_dir: &str,
    registry: &RegistryConfig,
) -> Result<()> {
    match &deployment.deployment_type {
        DeploymentType::Dockerfile => pull_dockerfile(deployment, conn, registry),
        DeploymentType::DockerCompose => pull_compose(deployment, conn, remote_dir),
    }
}

fn pull_dockerfile(
    deployment: &Deployment,
    conn: &SshConnection,
    registry: &RegistryConfig,
) -> Result<()> {
    let image = registry.image.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "No image name configured for this Dockerfile project.\n\
             Run `yolped setup registry` and provide an image name."
        )
    })?;

    let tag = registry.tags.first().map(|s| s.as_str()).unwrap_or("latest");
    let full_ref = format!("{}:{}", image, tag);

    logger::info(&format!("Pulling {}...", full_ref));
    println!();

    let code = conn.exec_stream(&format!("docker pull {}", shell_escape(&full_ref)))?;
    if code != 0 {
        bail!("docker pull failed for '{}' (exit code: {})", full_ref, code);
    }

    // Tag with the deployment name so `docker run --name <name> <name>` works.
    let code = conn.exec_stream(&format!(
        "docker tag {} {}",
        shell_escape(&full_ref),
        shell_escape(&deployment.name)
    ))?;
    if code != 0 {
        bail!("docker tag failed (exit code: {})", code);
    }

    println!();
    logger::success(&format!("Pulled and tagged {} as '{}'.", full_ref, deployment.name));
    Ok(())
}

fn pull_compose(
    deployment: &Deployment,
    conn: &SshConnection,
    remote_dir: &str,
) -> Result<()> {
    let remote_file = remote_file_path(deployment, remote_dir)?;

    logger::info(&format!("Pulling images for '{}'...", deployment.name));
    println!();

    let cmd = format!(
        "docker compose -f {} -p {} pull",
        shell_escape(&remote_file), shell_escape(&deployment.name)
    );

    let code = conn.exec_stream(&cmd)?;
    if code != 0 {
        bail!("docker compose pull failed (exit code: {})", code);
    }

    println!();
    logger::success(&format!("Images pulled for '{}'.", deployment.name));
    Ok(())
}
