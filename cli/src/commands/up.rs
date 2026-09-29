use anyhow::{bail, Result};

use crate::core::{
    logger,
    models::config::JdConfig,
};

use super::{deploy, push};

/// Build → push → deploy in a single command.
///
/// Requires both a registry and a server to be configured.
/// `version_override` replaces `@version` in tags for this run (see `push::run`).
pub fn run(extra_tags: &[String], version_override: Option<&str>) -> Result<()> {
    let config = JdConfig::load()?;

    if config.registry.is_none() {
        bail!(
            "No registry configured.\n\
             Run `yolped setup registry` to set one up, then `yolped up` again."
        );
    }

    if config.server.is_none() {
        bail!(
            "No remote server configured.\n\
             Run `yolped setup server` to add one, then `yolped up` again."
        );
    }

    logger::info("Step 1/3: Building...");
    println!();
    // Invoke the build step by re-using main logic via the runner
    {
        use std::env;
        use crate::core::{models::deployment::Deployment, registry::Registry, runner};
        let config_path = env::current_dir()?.join("yolped.json");
        let config = JdConfig::load()?;
        let platform = config.build.platform.clone();
        let deployment = Deployment::new(config.name, config.build.file)?;
        runner::build(&deployment, platform.as_deref())?;
        let mut registry = Registry::load()?;
        registry.mark_built(&config_path);
        registry.save()?;
    }

    println!();
    logger::info("Step 2/3: Pushing to registry...");
    println!();
    push::run(extra_tags, version_override)?;

    println!();
    logger::info("Step 3/3: Deploying...");
    println!();
    deploy::run(false, false, false)?;

    println!();
    logger::success("Deployed successfully!");
    Ok(())
}
