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
    cli::{Cli, Commands},
    core::{
        models::{
            config::JdConfig,
            deployment::{Deployment, ServerType},
        },
        registry::Registry,
        runner,
    },
};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Setup => commands::setup::run()?,
        Commands::Deploy { down, .. } => commands::deploy::run(down)?,
        Commands::Logs { name } => commands::logs::run(name.as_deref())?,
        Commands::List => commands::list::run()?,
        Commands::Build => {
            let config_path = std::env::current_dir()?.join("yolped.json");
            let config = JdConfig::load()?;
            let deployment = Deployment::new(
                config.name,
                config.file_path,
                String::new(),
                ServerType::Local,
            )?;
            runner::build(&deployment)?;
            let mut registry = Registry::load()?;
            registry.mark_built(&config_path);
            registry.save()?;
        }
    }

    Ok(())
}
