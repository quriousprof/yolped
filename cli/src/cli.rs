use clap::{Parser, Subcommand};

/// Yolped — Deployments made easy
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new Yolped configuration (yolped.json)
    Setup {
        #[command(subcommand)]
        subcommand: Option<SetupSubcommand>,
    },
    /// Build the deployment using the project's yolped.json config
    Build,
    /// Build and run the deployment on the local machine
    Deploy {
        /// Start the deployment (default)
        #[arg(long, conflicts_with = "down")]
        up: bool,
        /// Stop and remove the running containers
        #[arg(long, conflicts_with = "up")]
        down: bool,
        /// Force deployment on the local machine, ignoring server config
        #[arg(long)]
        local: bool,
        /// Remove existing images and force a clean rebuild before deploying
        #[arg(long)]
        rebuild: bool,
    },
    /// Stream logs for a deployment by name, or for the current directory's project
    Logs {
        /// Name of the deployment (defaults to current directory's yolped.json)
        name: Option<String>,
    },
    /// List all registered deployments
    List,
    /// Open an interactive SSH session to the configured remote server
    Ssh,
}

#[derive(Subcommand, Debug)]
pub enum SetupSubcommand {
    /// Reconfigure only the server settings for an existing project
    Server,
}
