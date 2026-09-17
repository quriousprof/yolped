use colored::Colorize;

use crate::core::{
    models::{config::JdConfig, deployment::{Deployment, DeploymentType, ServerType}},
    registry::{DeploymentStatus, Registry},
    runner,
};

pub fn run() -> anyhow::Result<()> {
    let mut registry = Registry::load()?;

    if registry.deployments.is_empty() {
        crate::core::logger::info(
            "No deployments registered. Run `jd setup` in a project directory.",
        );
        return Ok(());
    }

    // Live-check every entry and refresh status in the registry
    for entry in registry.deployments.iter_mut() {
        if entry.last_deployed_at.is_none() {
            continue; // never deployed — leave status as Unknown
        }
        if let Ok(config) = JdConfig::load_from(&entry.config_path) {
            if let Ok(dep) = Deployment::new(
                config.name,
                config.file_path,
                String::new(),
                ServerType::Local,
            ) {
                entry.status = runner::check_status(&dep);
            }
        }
    }
    registry.save()?;

    // Group entries by status
    let groups: &[(DeploymentStatus, &str)] = &[
        (DeploymentStatus::Running, "Running"),
        (DeploymentStatus::Errored, "Errored"),
        (DeploymentStatus::Stopped, "Stopped"),
        (DeploymentStatus::Unknown, "Not deployed"),
    ];

    println!();
    for (status, label) in groups {
        let entries: Vec<_> = registry
            .deployments
            .iter()
            .filter(|e| &e.status == status)
            .collect();

        if entries.is_empty() {
            continue;
        }

        let header = match status {
            DeploymentStatus::Running  => label.green().bold(),
            DeploymentStatus::Errored  => label.red().bold(),
            DeploymentStatus::Stopped  => label.yellow().bold(),
            DeploymentStatus::Unknown  => label.dimmed().bold(),
        };
        println!("  {}", header);

        for entry in entries {
            match JdConfig::load_from(&entry.config_path) {
                Ok(config) => {
                    let kind = match config.deployment_type {
                        DeploymentType::Dockerfile    => "Dockerfile",
                        DeploymentType::DockerCompose => "Docker Compose",
                    };
                    let fmt = |t: Option<chrono::DateTime<chrono::Utc>>| {
                        t.map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                            .unwrap_or_else(|| "never".to_string())
                    };

                    println!("    {} {}", "▸".cyan().bold(), config.name.bold());
                    println!("      project    {}", config.project_dir.display());
                    println!("      type       {}", kind);
                    println!("      config     {}", entry.config_path.display());
                    println!("      built      {}", fmt(entry.last_built_at));
                    println!("      deployed   {}", fmt(entry.last_deployed_at));
                    println!();
                }
                Err(_) => {
                    crate::core::logger::warn(&format!(
                        "Config missing or unreadable: {}",
                        entry.config_path.display()
                    ));
                }
            }
        }
    }

    Ok(())
}
