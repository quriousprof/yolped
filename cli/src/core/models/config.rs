use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::deployment::{DeploymentType, ServerType};

pub const CONFIG_FILE: &str = "yolped.json";

/// Persisted configuration written to yolped.json
#[derive(Serialize, Deserialize, Debug)]
pub struct JdConfig {
    pub name: String,
    pub version: String,
    pub project_dir: PathBuf,
    pub deployment_type: DeploymentType,
    pub file_path: PathBuf,
    pub server: ServerType,
    /// Arbitrary args appended to docker run / docker compose up at deploy time.
    /// Each entry is a single token e.g. ["-p", "8000:8000", "--restart", "unless-stopped"]
    #[serde(default)]
    pub deployment_args: Vec<String>,
}

impl JdConfig {
    pub fn load() -> Result<Self> {
        let path = PathBuf::from(CONFIG_FILE);
        if !path.exists() {
            bail!("No configuration file found for this project. Run `yolped setup` first.");
        }
        Self::load_from(&path)
    }

    pub fn load_from(path: &PathBuf) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read '{}'", path.display()))?;
        serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse '{}'", path.display()))
    }
}
