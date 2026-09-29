use std::{collections::HashMap, fs, path::PathBuf};

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

/// A single Dockerfile entry in the build pipeline.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BuildFile {
    /// Path to the Dockerfile.
    pub file: PathBuf,
    /// Fully qualified image name for this file (e.g. "ghcr.io/user/myapp-api").
    /// Falls back to `registry.image` when omitted (single-file projects).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Docker build arguments passed as `--build-arg KEY=VALUE`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub build_args: HashMap<String, String>,
}

/// What to build and for which target platform.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BuildConfig {
    /// Dockerfiles to build and push to the registry.
    /// Each entry may carry its own image name and build args.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<BuildFile>,
    /// Target platform for cross-compilation (e.g. "linux/amd64", "linux/arm64").
    /// Detected from the remote server during `yolped setup` and stored so
    /// `yolped build` can cross-compile without the server being reachable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

/// What to run on the server.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DeployConfig {
    /// Dockerfile or docker-compose.yml that defines the running service.
    /// Used by `yolped deploy`, `yolped logs`, and `yolped stop`.
    pub file: PathBuf,
    /// Extra args forwarded to `docker run` or `docker compose up`.
    /// e.g. ["-p", "8000:8000", "--restart", "unless-stopped"]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

/// Registry to push images to.
/// Credentials are never stored here — use `docker login` to authenticate.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegistryConfig {
    /// Fallback image name used when a BuildFile has no `image` set
    /// (e.g. "ghcr.io/user/myapp"). Not needed for docker-compose projects
    /// (each service carries its own `image:` field in the compose file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Tags to push. `@version` is replaced with `JdConfig::version` at push time.
    /// Defaults to ["latest", "@version"].
    #[serde(default = "default_tags")]
    pub tags: Vec<String>,
}

fn default_tags() -> Vec<String> {
    vec!["latest".to_string(), "@version".to_string()]
}

fn default_version() -> String {
    "0.1.0".to_string()
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
    /// Project version. Used as the `@version` placeholder in registry tags.
    /// e.g. tags: ["latest", "@version"] with version "1.2.3" → pushes "latest" and "1.2.3".
    #[serde(default = "default_version")]
    pub version: String,
    /// How to build images. Configure with `yolped setup` / `yolped setup registry`.
    pub build: BuildConfig,
    /// What to run on the server. Configure with `yolped setup`.
    pub deploy: DeployConfig,
    /// Registry for pushing images. Configure with `yolped setup registry`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<RegistryConfig>,
    /// Remote server to deploy to. None = deploy locally.
    /// Configure with `yolped setup server`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<ServerConfig>,
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
