use std::process::Command;

use anyhow::{bail, Result};

use crate::core::{
    logger,
    models::{
        config::{JdConfig, SshAuth},
    },
};

pub fn run() -> Result<()> {
    let config = JdConfig::load()?;

    let remote = config.server.ok_or_else(|| anyhow::anyhow!(
        "No remote server configured for this project. \
         Run `yolped setup server` to add one."
    ))?;

    logger::info(&format!("Connecting to {}@{}...", remote.user, remote.host));

    let mut cmd = Command::new("ssh");

    if let SshAuth::Key(ref key_path) = remote.auth {
        cmd.args(["-i", key_path.to_str().unwrap_or("")]);
    }

    cmd.arg(format!("{}@{}", remote.user, remote.host));

    let status = cmd.status()?;

    if !status.success() {
        bail!(
            "SSH exited with code {}",
            status.code().map_or_else(|| "unknown".to_string(), |c| c.to_string())
        );
    }

    Ok(())
}
