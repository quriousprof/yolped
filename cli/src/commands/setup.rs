use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::core::{
    logger,
    models::{
        config::JdConfig,
        deployment::{parse_file, ServerType},
    },
    registry::Registry,
    utils::generate_deployment_name,
};

pub fn run() -> Result<()> {
    logger::info("Setting up Yolped configuration...");
    println!();

    let name = {
        let default = generate_deployment_name();
        let input = prompt("Deployment name", Some(&default))?;
        if input.is_empty() { default } else { input }
    };

    let version = "0.1.0".to_string();

    let project_dir = {
        let cwd = env::current_dir().context("Failed to determine current directory")?;
        let default = cwd.to_string_lossy().into_owned();
        let input = prompt("Project directory", Some(&default))?;
        let path = PathBuf::from(if input.is_empty() { default } else { input });
        if !path.exists() {
            bail!("Directory '{}' does not exist", path.display());
        }
        path
    };

    let file_path = resolve_file_path(&project_dir)?;
    let deployment_type = parse_file(&file_path)?;

    let config = JdConfig {
        name,
        version,
        project_dir,
        deployment_type,
        file_path,
        server: ServerType::Local,
        deployment_args: vec![],
    };

    let config_path = env::current_dir()
        .context("Failed to determine current directory")?
        .join("yolped.json");

    if config_path.exists() {
        let backup = config_path.with_extension("json.old");
        fs::rename(&config_path, &backup).context("Failed to back up existing yolped.json")?;
        logger::info(&format!("Previous config saved to '{}'", backup.display()));
    }

    let json = serde_json::to_string_pretty(&config).context("Failed to serialize config")?;
    fs::write(&config_path, &json).context("Failed to write yolped.json")?;

    let mut registry = Registry::load()?;
    registry.upsert(config_path.clone());
    registry.save()?;

    println!();
    logger::success(&format!("Config saved to '{}'", config_path.display()));
    Ok(())
}

fn resolve_file_path(project_dir: &Path) -> Result<PathBuf> {
    let dockerfile = ["Dockerfile", "dockerfile"]
        .iter()
        .map(|f| project_dir.join(f))
        .find(|p| p.exists());

    let compose = [
        "docker-compose.yml",
        "docker-compose.yaml",
        "compose.yml",
        "compose.yaml",
    ]
    .iter()
    .map(|f| project_dir.join(f))
    .find(|p| p.exists());

    match (dockerfile, compose) {
        (Some(df), Some(dc)) => {
            logger::info(&format!(
                "Found '{}' and '{}'",
                df.display(),
                dc.display()
            ));
            let df_s = df.to_string_lossy().into_owned();
            let dc_s = dc.to_string_lossy().into_owned();
            let opts = [df_s.as_str(), dc_s.as_str(), "Enter a custom path"];
            match prompt_choice("Which would you like to use?", &opts, 0)? {
                0 => Ok(df),
                1 => Ok(dc),
                _ => ask_custom_path(),
            }
        }
        (Some(df), None) => {
            let df_s = df.to_string_lossy().into_owned();
            logger::info(&format!("Detected '{}'", df.display()));
            let opts = [df_s.as_str(), "Enter a custom path"];
            match prompt_choice("Use this file?", &opts, 0)? {
                0 => Ok(df),
                _ => ask_custom_path(),
            }
        }
        (None, Some(dc)) => {
            let dc_s = dc.to_string_lossy().into_owned();
            logger::info(&format!("Detected '{}'", dc.display()));
            let opts = [dc_s.as_str(), "Enter a custom path"];
            match prompt_choice("Use this file?", &opts, 0)? {
                0 => Ok(dc),
                _ => ask_custom_path(),
            }
        }
        (None, None) => {
            logger::warn("No Dockerfile or docker-compose file found in the project directory.");
            ask_custom_path()
        }
    }
}

fn ask_custom_path() -> Result<PathBuf> {
    loop {
        let input = prompt("Path to Dockerfile or docker-compose file", None)?;
        if input.is_empty() {
            logger::warn("Path cannot be empty.");
            continue;
        }
        let path = PathBuf::from(&input);
        if path.exists() {
            return Ok(path);
        }
        logger::warn(&format!("'{}' not found, try again.", path.display()));
    }
}

fn prompt(question: &str, default: Option<&str>) -> Result<String> {
    match default {
        Some(d) => print!("{} [{}]: ", question, d),
        None => print!("{}: ", question),
    }
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn prompt_choice(question: &str, options: &[&str], default_idx: usize) -> Result<usize> {
    println!("{}", question);
    for (i, opt) in options.iter().enumerate() {
        if i == default_idx {
            println!("  [{}] {} (default)", i + 1, opt);
        } else {
            println!("  [{}] {}", i + 1, opt);
        }
    }

    loop {
        print!("Choice [{}]: ", default_idx + 1);
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();

        if trimmed.is_empty() {
            return Ok(default_idx);
        }

        if let Ok(n) = trimmed.parse::<usize>() {
            if n >= 1 && n <= options.len() {
                return Ok(n - 1);
            }
        }

        logger::warn(&format!(
            "Enter a number between 1 and {}.",
            options.len()
        ));
    }
}
