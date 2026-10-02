use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use rustyline::{CompletionType, Config, Editor, Helper, Result as RlResult};
use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::Context as RlContext;

/// Minimal rustyline helper that only provides filename tab-completion.
struct FileCompleterHelper {
    completer: FilenameCompleter,
}

impl Helper for FileCompleterHelper {}

impl Completer for FileCompleterHelper {
    type Candidate = Pair;
    fn complete(&self, line: &str, pos: usize, ctx: &RlContext<'_>) -> RlResult<(usize, Vec<Pair>)> {
        self.completer.complete(line, pos, ctx)
    }
}

impl Hinter for FileCompleterHelper {
    type Hint = String;
}

impl Highlighter for FileCompleterHelper {}
impl Validator for FileCompleterHelper {}

use anyhow::{Context, Result, bail};

use crate::core::{
    logger,
    models::{
        config::{BuildConfig, DeployConfig, JdConfig, RegistryConfig, ServerConfig, SshAuth},
        deployment::parse_file,
    },
    registry::Registry,
    remote_runner,
    ssh::SshConnection,
    utils::generate_deployment_name,
};

pub fn run() -> Result<()> {
    logger::info("Setting up Yolped configuration...");
    println!();

    let name = {
        let default = generate_deployment_name();
        loop {
            let input = prompt("Deployment name", Some(&default))?;
            let candidate = if input.is_empty() { default.clone() } else { input };
            if is_valid_name(&candidate) {
                break candidate;
            }
            logger::warn("Name may only contain letters, numbers, hyphens, underscores, and dots.");
        }
    };

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
    // Validate the file can be parsed
    parse_file(&file_path)?;

    let server = prompt_server()?;

    // Detect the remote platform so `yolped build` can cross-compile without
    // needing the server to be reachable at build time.
    let platform = if let Some(ref s) = server {
        println!();
        logger::info("Detecting remote platform...");
        match SshConnection::connect(s) {
            Ok(conn) => {
                let p = remote_runner::detect_platform(&conn);
                logger::success(&format!("Remote platform: {}", p));
                Some(p)
            }
            Err(e) => {
                logger::warn(&format!("Could not detect platform ({}). You can set it later.", e));
                None
            }
        }
    } else {
        None
    };

    let config = JdConfig {
        name,
        version: "0.1.0".to_string(),
        build: BuildConfig { files: vec![], platform },
        deploy: DeployConfig { file: file_path, args: vec![] },
        server,
        registry: None,
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

/// Configure or update the registry section of an existing yolped.json
pub fn run_registry() -> Result<()> {
    use crate::core::models::deployment::{parse_file, DeploymentType};

    let config_path = env::current_dir()
        .context("Failed to determine current directory")?
        .join("yolped.json");

    if !config_path.exists() {
        bail!("No yolped.json found. Run `yolped setup` first.");
    }

    let mut config = JdConfig::load()?;
    let is_compose = matches!(parse_file(&config.deploy.file), Ok(DeploymentType::DockerCompose));

    println!();
    logger::info("Configure the registry to push images to.");
    logger::info("Credentials are not stored — run `docker login <registry>` separately.");
    println!();

    // For compose projects the image name lives in each service's `image:` field,
    // so we don't need (or use) a top-level image name.
    let image = if is_compose {
        logger::info(
            "docker-compose project: image names are taken from each service's 'image:' field.\n\
             Make sure each service you want to push has 'build:' and 'image:' set."
        );
        println!();
        None
    } else {
        let current = config
            .registry
            .as_ref()
            .and_then(|r| r.image.as_deref())
            .unwrap_or("");
        let input = if current.is_empty() {
            prompt("Image name (e.g. ghcr.io/user/myapp)", None)?
        } else {
            prompt("Image name", Some(current))?
        };
        let candidate = if input.is_empty() { current.to_string() } else { input };
        if candidate.is_empty() {
            bail!("Image name cannot be empty for Dockerfile projects.");
        }
        Some(candidate)
    };

    let tags = {
        let current = config
            .registry
            .as_ref()
            .map(|r| r.tags.join(", "))
            .unwrap_or_else(|| "latest".to_string());
        let input = prompt("Tags (comma-separated)", Some(&current))?;
        let raw = if input.is_empty() { current } else { input };
        raw.split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
    };

    if tags.is_empty() {
        bail!("At least one tag is required.");
    }

    config.registry = Some(RegistryConfig { image, tags });

    let json = serde_json::to_string_pretty(&config).context("Failed to serialize config")?;
    fs::write(&config_path, &json).context("Failed to write yolped.json")?;

    println!();
    logger::success("Registry configuration saved.");
    logger::info("Run `yolped push` to build and push your image.");
    Ok(())
}

/// Only reconfigure the server section of an existing yolped.json
pub fn run_server() -> Result<()> {
    let config_path = env::current_dir()
        .context("Failed to determine current directory")?
        .join("yolped.json");

    if !config_path.exists() {
        bail!("No yolped.json found. Run `yolped setup` first.");
    }

    let mut config = JdConfig::load()?;
    config.server = prompt_server()?;

    // Re-detect platform for the new server
    if let Some(ref s) = config.server {
        println!();
        logger::info("Detecting remote platform...");
        match SshConnection::connect(s) {
            Ok(conn) => {
                let p = remote_runner::detect_platform(&conn);
                logger::success(&format!("Remote platform: {}", p));
                config.build.platform = Some(p);
            }
            Err(e) => {
                logger::warn(&format!("Could not detect platform ({}). Platform unchanged.", e));
            }
        }
    } else {
        config.build.platform = None;
    }

    let json = serde_json::to_string_pretty(&config).context("Failed to serialize config")?;
    fs::write(&config_path, &json).context("Failed to write yolped.json")?;

    println!();
    logger::success("Server configuration updated.");
    Ok(())
}

/// Returns true if the name is safe to use as a Docker container/image name
/// and in shell commands: letters, numbers, hyphens, underscores, and dots only.
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn prompt_server() -> Result<Option<ServerConfig>> {
    println!();
    let choice = prompt_choice("Server", &["Local (default)", "Remote"], 0)?;

    if choice == 0 {
        return Ok(None);
    }

    let host = {
        let input = prompt("IP address or hostname", None)?;
        if input.is_empty() {
            bail!("Host cannot be empty");
        }
        input
    };

    let user = {
        let input = prompt("SSH user", Some("root"))?;
        if input.is_empty() { "root".to_string() } else { input }
    };

    let auth_choice = prompt_choice("Auth method", &["Password", "SSH key file"], 0)?;
    let auth = if auth_choice == 0 {
        logger::info("You will be prompted for your password at deploy time — it is never stored.");
        SshAuth::Password
    } else {
        prompt_key_path()?
    };

    let remote_dir = {
        let input = prompt("Remote deployment directory", Some("~/deployments"))?;
        if input.is_empty() { "~/deployments".to_string() } else { input }
    };

    Ok(Some(ServerConfig { host, user, auth, remote_dir }))
}

/// Detect Dockerfile and docker-compose files in a project directory.
fn detect_docker_files(project_dir: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
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

    (dockerfile, compose)
}

fn resolve_file_path(project_dir: &Path) -> Result<PathBuf> {
    let (dockerfile, compose) = detect_docker_files(project_dir);

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

/// Prompt for an SSH private key path with tab-completion for file names.
fn prompt_key_path() -> Result<SshAuth> {
    let rl_config = Config::builder()
        .completion_type(CompletionType::List)
        .build();

    let mut rl = Editor::with_config(rl_config)
        .context("Failed to initialize file completer")?;
    rl.set_helper(Some(FileCompleterHelper { completer: FilenameCompleter::new() }));

    logger::info("Tab to autocomplete. Press Enter to confirm.");

    loop {
        match rl.readline("Path to SSH private key: ") {
            Ok(input) => {
                let input = input.trim().to_string();
                if input.is_empty() {
                    logger::warn("Path cannot be empty.");
                    continue;
                }
                let raw = PathBuf::from(&input);
                let path = if raw.is_absolute() {
                    raw
                } else {
                    env::current_dir()?.join(&raw)
                };
                if !path.exists() {
                    logger::warn(&format!("'{}' not found, try again.", path.display()));
                    continue;
                }
                return Ok(SshAuth::Key(path));
            }
            Err(rustyline::error::ReadlineError::Interrupted)
            | Err(rustyline::error::ReadlineError::Eof) => {
                bail!("Aborted.");
            }
            Err(e) => return Err(e.into()),
        }
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
