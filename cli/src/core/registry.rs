use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const REGISTRY_DIR: &str = ".yolped";
const REGISTRY_FILE: &str = "registry.json";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentStatus {
    Running,
    Stopped,
    Errored,
    #[default]
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegistryEntry {
    pub config_path: PathBuf,
    pub registered_at: DateTime<Utc>,
    pub last_built_at: Option<DateTime<Utc>>,
    pub last_deployed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub status: DeploymentStatus,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Registry {
    pub deployments: Vec<RegistryEntry>,
}

impl Registry {
    pub fn path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(REGISTRY_DIR).join(REGISTRY_FILE))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = fs::read_to_string(&path).context("Failed to read registry")?;
        serde_json::from_str(&contents).context("Failed to parse registry")
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .context("Failed to create ~/.yolped directory")?;
        }
        let json =
            serde_json::to_string_pretty(self).context("Failed to serialize registry")?;
        fs::write(&path, json).context("Failed to write registry")?;
        Ok(())
    }

    /// Add a new entry if not already tracked (keyed by config_path).
    pub fn upsert(&mut self, config_path: PathBuf) {
        let exists = self.deployments.iter().any(|e| e.config_path == config_path);
        if !exists {
            self.deployments.push(RegistryEntry {
                config_path,
                registered_at: Utc::now(),
                last_built_at: None,
                last_deployed_at: None,
                status: DeploymentStatus::Unknown,
            });
        }
    }

    pub fn update_status(&mut self, config_path: &PathBuf, status: DeploymentStatus) {
        if let Some(entry) = self.deployments.iter_mut().find(|e| &e.config_path == config_path) {
            entry.status = status;
        }
    }

    pub fn mark_deployed(&mut self, config_path: &PathBuf) {
        if let Some(entry) = self.deployments.iter_mut().find(|e| &e.config_path == config_path) {
            entry.last_deployed_at = Some(Utc::now());
        }
    }

    pub fn mark_built(&mut self, config_path: &PathBuf) {
        if let Some(entry) = self.deployments.iter_mut().find(|e| &e.config_path == config_path) {
            entry.last_built_at = Some(Utc::now());
        }
    }
}
