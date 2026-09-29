use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "yolped.json";

/// How to authenticate to the remote server.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SshAuth {
    /// Password is never stored — user is prompted at connect time
    Password,
    /// Absolute path to SSH private key file
    Key(PathBuf),
}

/// What to build and for which target platform.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BuildConfig {
    /// Path to the Dockerfile or docker-compose file.
    pub file: PathBuf,
    /// Target platform for cross-compilation (e.g. "linux/amd64", "linux/arm64").
    /// Detected from the remote server during `yolped setup` and stored so
    /// `yolped build` can cross-compile without the server being reachable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

/// Registry to push images to.
/// Credentials are never stored here — use `docker login` to authenticate.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegistryConfig {
    /// Fully qualified image name without a tag, e.g. "ghcr.io/user/myapp".
    /// Required for Dockerfile projects; not needed for docker-compose projects
    /// (each service carries its own `image:` field in the compose file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Tags to push. Defaults to ["latest"].
    #[serde(default = "default_tags")]
    pub tags: Vec<String>,
}

fn default_tags() -> Vec<String> {
    vec!["latest".to_string()]
}

/// Remote server to deploy to.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerConfig {
    /// Hostname or IP address.
    pub host: String,
    pub user: String,
    pub auth: SshAuth,
    pub remote_dir: String,
}

/// Persisted project configuration — written to yolped.json.
#[derive(Serialize, Deserialize, Debug)]
pub struct JdConfig {
    pub name: String,
    pub build: BuildConfig,
    /// Registry for pushing images. Configure with `yolped setup registry`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<RegistryConfig>,
    /// Remote server to deploy to. None = deploy locally.
    /// Configure with `yolped setup server`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<ServerConfig>,
    /// Extra args passed to `docker run` or `docker compose up` at deploy time.
    /// e.g. ["-p", "8000:8000", "--restart", "unless-stopped"]
    #[serde(default)]
    pub run_args: Vec<String>,
}

impl JdConfig {
    pub fn load() -> Result<Self> {
        let path = PathBuf::from(CONFIG_FILE);
        if !path.exists() {
            bail!("No yolped.json found in this directory. Run `yolped setup` first.");
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
