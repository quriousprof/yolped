use std::{
    collections::HashMap,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{bail, Context, Result};

use crate::core::{
    logger,
    models::{
        config::{JdConfig, RegistryConfig},
        deployment::{parse_file, DeploymentType},
    },
};

/// Build and push all images.
///
/// `version_override` replaces `@version` in tags for this run only (does not
/// modify yolped.json). If None, `config.version` is used.
pub fn run(extra_tags: &[String], version_override: Option<&str>) -> Result<()> {
    let config = JdConfig::load()?;

    let registry = config.registry.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "No registry configured.\n\
             Run `yolped setup registry` to set one up."
        )
    })?;

    let version = version_override.unwrap_or(&config.version);

    // Resolve @version in every configured tag, then merge in extra_tags.
    let mut tags: Vec<String> = registry
        .tags
        .iter()
        .map(|t| resolve_version(t, version))
        .collect();

    for t in extra_tags {
        let resolved = resolve_version(t, version);
        if !tags.contains(&resolved) {
            tags.push(resolved);
        }
    }

    // If the user passed --version, also add the raw version string as a tag
    // (so `yolped push --version v1.2.3` always produces a v1.2.3 tag even if
    // @version wasn't in the configured tag list).
    if let Some(v) = version_override {
        let v = v.to_string();
        if !tags.contains(&v) {
            tags.push(v);
        }
    }

    let platform = config.build.platform.as_deref();

    if config.build.files.is_empty() {
        // ── Backward-compat: single-file project ────────────────────────────
        let deploy_type = parse_file(&config.build.file)?;
        match deploy_type {
            DeploymentType::Dockerfile => {
                let image = registry.image.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "No image name configured.\n\
                         Run `yolped setup registry` and provide an image name."
                    )
                })?;
                push_single(
                    &config.build.file,
                    image,
                    &config.name,
                    &tags,
                    platform,
                    &HashMap::new(),
                )?;
            }
            DeploymentType::DockerCompose => {
                push_compose(&config.build.file, &config.name, &registry)?;
            }
        }
    } else {
        // ── Multi-file: build and push each entry ────────────────────────────
        let n = config.build.files.len();
        for (i, bf) in config.build.files.iter().enumerate() {
            let image = bf
                .image
                .as_deref()
                .or(registry.image.as_deref())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "No image name for '{}'. \
                         Set `image` on the build file entry or run `yolped setup registry`.",
                        bf.file.display()
                    )
                })?;

            if n > 1 {
                logger::info(&format!(
                    "[{}/{}] {}",
                    i + 1,
                    n,
                    bf.file.display()
                ));
            }

            push_single(&bf.file, image, &config.name, &tags, platform, &bf.build_args)?;
            println!();
        }
    }

    Ok(())
}

/// Build a single Dockerfile and push it with all `tags`.
fn push_single(
    file_path: &Path,
    image: &str,
    local_name: &str,
    tags: &[String],
    platform: Option<&str>,
    build_args: &HashMap<String, String>,
) -> Result<()> {
    let context_path = file_path
        .parent()
        .context("Could not determine build context: Dockerfile has no parent directory")?;

    let file_str = file_path
        .to_str()
        .context("Dockerfile path contains invalid UTF-8")?;
    let context_str = context_path
        .to_str()
        .context("Build context path contains invalid UTF-8")?;

    logger::info(&format!("Building {}...", file_path.display()));
    println!();

    let mut cmd = Command::new("docker");

    if platform.is_some() {
        cmd.args(["buildx", "build", "--load"]);
    } else {
        cmd.args(["build"]);
    }

    cmd.args(["-f", file_str]);

    if let Some(p) = platform {
        cmd.args(["--platform", p]);
    }

    // Tag with the local deployment name so `yolped deploy` can still find it
    cmd.args(["-t", local_name]);

    // Tag with every registry ref up front — Docker only builds once
    for tag in tags {
        cmd.args(["-t", &format!("{}:{}", image, tag)]);
    }

    for (k, v) in build_args {
        cmd.args(["--build-arg", &format!("{}={}", k, v)]);
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

fn push_compose(file_path: &Path, project_name: &str, registry: &RegistryConfig) -> Result<()> {
    let file_str = file_path
        .to_str()
        .context("Compose file path contains invalid UTF-8")?;

    logger::info(&format!("Building '{}' (compose)...", project_name));
    println!();

    let status = Command::new("docker")
        .args(["compose", "-f", file_str, "-p", project_name, "build"])
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
        .args(["compose", "-f", file_str, "-p", project_name, "push"])
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
    logger::success(&format!("'{}' pushed successfully.", project_name));
    Ok(())
}

/// Replace `@version` in a tag string with the resolved version.
pub fn resolve_version(tag: &str, version: &str) -> String {
    tag.replace("@version", version)
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
