use std::path::PathBuf;

use chrono::{DateTime, Utc};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Represents a single deployment configuration
#[derive(Serialize, Deserialize, Debug)]
pub struct Deployment {
    pub(crate) name: String,
    pub(crate) file_path: PathBuf,
    pub(crate) config_location: String,
    pub(crate) deployment_type: DeploymentType,
    pub(crate) server: ServerType,
    pub(crate) deployed_at: DateTime<Utc>,
    pub(crate) status: DeploymentStatus,
}

impl Deployment {
    pub fn new(
        name: String,
        file_path: PathBuf,
        config_location: String,
        server: ServerType,
    ) -> Result<Self> {
        let deployment_type = parse_file(&file_path)?;
        Ok(Self {
            name,
            file_path,
            config_location,
            deployment_type,
            server,
            deployed_at: Utc::now(),
            status: DeploymentStatus::Idle,
        })
    }
}

/// Determine the [`DeploymentType`] from a file path
pub fn parse_file(file_path: &PathBuf) -> Result<DeploymentType> {
    if !file_path.exists() {
        bail!("File '{}' does not exist", file_path.display());
    }

    match file_path.extension() {
        Some(ext) if ext == "yml" || ext == "yaml" => {
            let filename = file_path
                .file_name()
                .and_then(|n| n.to_str())
                .context("Filename contains invalid UTF-8")?;

            if filename.contains("compose") {
                Ok(DeploymentType::DockerCompose)
            } else {
                bail!(
                    "YAML file '{}' is not a docker-compose file (filename must contain 'compose')",
                    filename
                )
            }
        }
        Some(ext) => bail!(
            "Unsupported file extension '.{}': only Dockerfiles and docker-compose YAML files are supported",
            ext.to_string_lossy()
        ),
        None => {
            let filename = file_path
                .file_name()
                .and_then(|n| n.to_str())
                .context("Filename contains invalid UTF-8")?;

            if filename == "Dockerfile" {
                Ok(DeploymentType::Dockerfile)
            } else {
                bail!(
                    "File '{}' is not a valid Dockerfile (expected filename 'Dockerfile')",
                    filename
                )
            }
        }
    }
}

/// Type of deployment
#[derive(Serialize, Deserialize, Debug)]
pub enum DeploymentType {
    Dockerfile,
    DockerCompose,
}

/// Target server for deployment
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServerType {
    Local,
    Remote(RemoteServer),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RemoteServer {
    pub ip: String,
    pub user: String,
    pub auth: SshAuth,
    pub remote_dir: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SshAuth {
    /// Password is never stored — user is prompted at deploy time
    Password,
    /// Absolute path to SSH private key file
    Key(PathBuf),
}

/// Lifecycle status of a deployment
#[derive(Serialize, Deserialize, Debug)]
pub enum DeploymentStatus {
    Idle,
    Starting,
    Running,
    Stopped,
}
