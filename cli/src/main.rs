pub mod app;
pub mod cli;
pub mod commands;
pub mod core;
pub mod event;
pub mod tui;
pub mod ui;
pub mod update;

use anyhow::Result;
use clap::Parser;

use crate::{
    cli::{Cli, Commands, SetupSubcommand},
    commands::{push, up},
    core::{
        models::{
            config::JdConfig,
            deployment::Deployment,
        },
        registry::Registry,
        runner,
    },
};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Setup { subcommand } => match subcommand {
            None => commands::setup::run()?,
            Some(SetupSubcommand::Server) => commands::setup::run_server()?,
            Some(SetupSubcommand::Registry) => commands::setup::run_registry()?,
        },
        Commands::Push { tags, version } => {
            push::run(&tags, version.as_deref())?;
        }
        Commands::Up { tags, version } => {
            up::run(&tags, version.as_deref())?;
        }
        Commands::Deploy { down, local, rebuild, .. } => commands::deploy::run(down, local, rebuild)?,
        Commands::Logs { name } => commands::logs::run(name.as_deref())?,
        Commands::List => commands::list::run()?,
        Commands::Ssh => commands::ssh::run()?,
        Commands::Build => {
            let config_path = std::env::current_dir()?.join("yolped.json");
            let config = JdConfig::load()?;
            let platform = config.build.platform.clone();

            if config.build.files.is_empty() {
                // No explicit build files — build the deploy target
                let deployment = Deployment::new(config.name, config.deploy.file)?;
                runner::build(&deployment, platform.as_deref())?;
            } else {
                // Multi-file: build each entry in build.files
                for bf in &config.build.files {
                    runner::build_file(
                        &bf.file,
                        bf.image.as_deref().unwrap_or(&config.name),
                        platform.as_deref(),
                        &bf.build_args,
                    )?;
                }
            }

            let mut registry = Registry::load()?;
            registry.mark_built(&config_path);
            registry.save()?;
        }
    }

    Ok(())
}
