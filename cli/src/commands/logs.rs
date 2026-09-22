use std::env;

use anyhow::{bail, Result};

use crate::core::{
    models::{
        config::JdConfig,
        deployment::{Deployment, ServerType},
    },
    registry::Registry,
    remote_runner,
    runner,
    ssh::SshConnection,
};

pub fn run(name: Option<&str>) -> Result<()> {
    match name {
        Some(name) => logs_by_name(name),
        None => logs_current_dir(),
    }
}

fn logs_current_dir() -> Result<()> {
    let config = JdConfig::load()?;
    stream_logs(config)
}

fn logs_by_name(name: &str) -> Result<()> {
    let registry = Registry::load()?;

    for entry in &registry.deployments {
        match JdConfig::load_from(&entry.config_path) {
            Ok(config) if config.name == name => return stream_logs(config),
            _ => continue,
        }
    }

    bail!(
        "No deployment named '{}' found. Run `yolped list` to see all deployments.",
        name
    )
}

fn stream_logs(config: JdConfig) -> Result<()> {
    let server = config.server.clone();
    let deployment = Deployment::new(
        config.name,
        config.file_path,
        String::new(),
        server.clone(),
    )?;

    match server {
        ServerType::Local => runner::logs(&deployment),
        ServerType::Remote(ref remote) => {
            let conn = SshConnection::connect(remote)?;
            let remote_dir = conn.expand_path(&remote.remote_dir)?;
            remote_runner::logs(&deployment, &conn, &remote_dir)
        }
    }
}
