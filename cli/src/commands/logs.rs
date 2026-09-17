use std::env;

use anyhow::{bail, Result};

use crate::core::{
    models::{
        config::JdConfig,
        deployment::{Deployment, ServerType},
    },
    registry::Registry,
    runner,
};

pub fn run(name: Option<&str>) -> Result<()> {
    let deployment = match name {
        Some(name) => find_by_name(name)?,
        None => from_current_dir()?,
    };
    runner::logs(&deployment)
}

fn from_current_dir() -> Result<Deployment> {
    let config = JdConfig::load()?;
    Deployment::new(config.name, config.file_path, String::new(), ServerType::Local)
}

fn find_by_name(name: &str) -> Result<Deployment> {
    let registry = Registry::load()?;

    for entry in &registry.deployments {
        match JdConfig::load_from(&entry.config_path) {
            Ok(config) if config.name == name => {
                return Deployment::new(
                    config.name,
                    config.file_path,
                    String::new(),
                    ServerType::Local,
                );
            }
            _ => continue,
        }
    }

    bail!(
        "No deployment named '{}' found. Run `yolped list` to see all deployments.",
        name
    )
}
