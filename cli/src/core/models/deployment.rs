use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Represents the core information needed to build/deploy a project.
#[derive(Debug)]
pub struct Deployment {
    pub name: String,
    pub file_path: PathBuf,
    pub deployment_type: DeploymentType,
}

impl Deployment {
    pub fn new(name: String, file_path: PathBuf) -> Result<Self> {
        let deployment_type = parse_file(&file_path)?;
        Ok(Self { name, file_path, deployment_type })
    }
}

/// Determine the [`DeploymentType`] from a file path.
pub fn parse_file(file_path: &Path) -> Result<DeploymentType> {
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

            if filename == "Dockerfile" || filename == "dockerfile" {
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
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum DeploymentType {
    Dockerfile,
    DockerCompose,
}
