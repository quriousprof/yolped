use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::core::{
    logger,
    models::{
        config::{JdConfig, RegistryConfig},
        deployment::{Deployment, DeploymentType},
    },
};

pub fn run(extra_tags: &[String]) -> Result<()> {
    let config = JdConfig::load()?;

    let registry = config.registry.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "No registry configured.\n\
             Run `yolped setup registry` to set one up."
        )
    })?;

    // Merge configured tags with any extra ones passed via --tag
    let mut tags = registry.tags.clone();
    for t in extra_tags {
        if !tags.contains(t) {
            tags.push(t.clone());
        }
    }

    let deployment = Deployment::new(config.name, config.build.file)?;

    match &deployment.deployment_type {
        DeploymentType::Dockerfile => push_dockerfile(&deployment, &registry, &tags),
        DeploymentType::DockerCompose => push_compose(&deployment, &registry, &tags),
    }
}

fn push_dockerfile(deployment: &Deployment, registry: &RegistryConfig, tags: &[String]) -> Result<()> {
    let image = registry.image.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "No image name configured for this Dockerfile project.\n\
             Run `yolped setup registry` and provide an image name (e.g. ghcr.io/user/myapp)."
        )
    })?;

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

    logger::info(&format!("Building '{}'...", deployment.name));
    println!();

    let mut cmd = Command::new("docker");
    cmd.args(["build", "-f", file_str]);
    // Keep the local image name so `yolped deploy` can still find it
    cmd.args(["-t", &deployment.name]);
    for tag in tags {
        cmd.args(["-t", &format!("{}:{}", image, tag)]);
    }
    cmd.arg(context_str)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd
        .spawn()
        .context("Failed to spawn 'docker build'. Is Docker installed and running?")?
        .wait()
        .context("Failed to wait for 'docker build'")?;

    if !status.success() {
        bail!(
            "docker build failed (exit code: {})",
            status.code().map_or_else(|| "unknown".to_string(), |c| c.to_string())
        );
    }

    println!();

    for tag in tags {
        let full_ref = format!("{}:{}", image, tag);
        logger::info(&format!("Pushing {}...", full_ref));
        println!();

        let status = Command::new("docker")
            .args(["push", &full_ref])
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn 'docker push'")?
            .wait()
            .context("Failed to wait for 'docker push'")?;

        if !status.success() {
            bail!(
                "docker push failed for '{}' (exit code: {}).\n\
                 If this is an auth error, log in first:\n\n  \
                 docker login {}",
                full_ref,
                status.code().map_or_else(|| "unknown".to_string(), |c| c.to_string()),
                registry_host(image)
            );
        }

        println!();
        logger::success(&format!("Pushed {}.", full_ref));
    }

    Ok(())
}

fn push_compose(deployment: &Deployment, registry: &RegistryConfig, _tags: &[String]) -> Result<()> {
    let file_str = deployment
        .file_path
        .to_str()
        .context("Compose file path contains invalid UTF-8")?;

    logger::info(&format!("Building '{}' (compose)...", deployment.name));
    println!();

    let status = Command::new("docker")
        .args(["compose", "-f", file_str, "-p", &deployment.name, "build"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to spawn 'docker compose build'")?
        .wait()
        .context("Failed to wait for 'docker compose build'")?;

    if !status.success() {
        bail!(
            "docker compose build failed (exit code: {})",
            status.code().map_or_else(|| "unknown".to_string(), |c| c.to_string())
        );
    }

    println!();
    logger::info("Pushing images...");
    println!();

    let status = Command::new("docker")
        .args(["compose", "-f", file_str, "-p", &deployment.name, "push"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to spawn 'docker compose push'")?
        .wait()
        .context("Failed to wait for 'docker compose push'")?;

    if !status.success() {
        let host = registry.image.as_deref().map(registry_host).unwrap_or("your registry");
        bail!(
            "docker compose push failed (exit code: {}).\n\
             Make sure each service in your compose file has an 'image:' field pointing to the registry.\n\
             If this is an auth error, log in first:\n\n  \
             docker login {}",
            status.code().map_or_else(|| "unknown".to_string(), |c| c.to_string()),
            host
        );
    }

    println!();
    logger::success(&format!("'{}' pushed successfully.", deployment.name));
    Ok(())
}

/// Extract the registry host from an image reference for helpful error messages.
/// "ghcr.io/user/image" → "ghcr.io", "user/image" → "docker.io"
fn registry_host(image: &str) -> &str {
    let first = image.split('/').next().unwrap_or("");
    if first.contains('.') || first.contains(':') {
        first
    } else {
        "docker.io"
    }
}
