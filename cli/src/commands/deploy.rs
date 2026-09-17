use std::{env, path::PathBuf};

use anyhow::Result;

use crate::core::{
    logger,
    models::{
        config::JdConfig,
        deployment::{Deployment, DeploymentType, ServerType},
    },
    registry::{DeploymentStatus, Registry},
    runner,
};

pub fn run(down: bool) -> Result<()> {
    let config_path = env::current_dir()?.join("yolped.json");
    let config = JdConfig::load()?;
    let deployment_args = config.deployment_args.clone();

    let deployment = Deployment::new(
        config.name,
        config.file_path,
        String::new(),
        ServerType::Local,
    )?;

    if down {
        runner::stop(&deployment)?;
        let mut registry = Registry::load()?;
        registry.update_status(&config_path, DeploymentStatus::Stopped);
        registry.save()?;
    } else {
        ensure_built(&config_path, &deployment)?;
        let deploy_result = runner::deploy(&deployment, &deployment_args);
        let status = match &deploy_result {
            Ok(_) => runner::check_status(&deployment),
            Err(_) => DeploymentStatus::Errored,
        };
        let mut registry = Registry::load()?;
        registry.mark_deployed(&config_path);
        registry.update_status(&config_path, status);
        registry.save()?;
        deploy_result?;
    }

    Ok(())
}

fn ensure_built(config_path: &PathBuf, deployment: &Deployment) -> Result<()> {
    let is_built = match &deployment.deployment_type {
        // For Dockerfile, ask Docker directly — the image may have been removed manually
        DeploymentType::Dockerfile => runner::image_exists(&deployment.name),
        // For Compose, image names aren't predictable, so rely on the registry
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
