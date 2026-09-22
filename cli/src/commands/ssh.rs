use std::process::Command;

use anyhow::{bail, Result};

use crate::core::{
    logger,
    models::{
        config::JdConfig,
        deployment::{ServerType, SshAuth},
    },
};

pub fn run() -> Result<()> {
    let config = JdConfig::load()?;

    let remote = match config.server {
        ServerType::Remote(r) => r,
        ServerType::Local => bail!(
            "No remote server configured for this project. \
             Run `yolped setup server` to add one."
        ),
    };

    logger::info(&format!("Connecting to {}@{}...", remote.user, remote.ip));

    let mut cmd = Command::new("ssh");

    if let SshAuth::Key(ref key_path) = remote.auth {
        cmd.args(["-i", key_path.to_str().unwrap_or("")]);
    }

    cmd.arg(format!("{}@{}", remote.user, remote.ip));

    let status = cmd.status()?;

    if !status.success() {
        bail!(
            "SSH exited with code {}",
            status.code().map_or_else(|| "unknown".to_string(), |c| c.to_string())
        );
    }

    Ok(())
}
