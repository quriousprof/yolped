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
        Commands::Push { tags } => push::run(&tags)?,
        Commands::Up { tags } => up::run(&tags)?,
        Commands::Deploy { down, local, rebuild, .. } => commands::deploy::run(down, local, rebuild)?,
        Commands::Logs { name } => commands::logs::run(name.as_deref())?,
        Commands::List => commands::list::run()?,
        Commands::Ssh => commands::ssh::run()?,
        Commands::Build => {
            let config_path = std::env::current_dir()?.join("yolped.json");
            let config = JdConfig::load()?;
            let platform = config.build.platform.clone();
            let deployment = Deployment::new(config.name, config.build.file)?;
            runner::build(&deployment, platform.as_deref())?;
            let mut registry = Registry::load()?;
            registry.mark_built(&config_path);
            registry.save()?;
        }
    }

    Ok(())
}
