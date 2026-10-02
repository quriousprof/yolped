# yolped

A Rust CLI tool for managing Docker and Docker Compose deployments directly from the terminal. Built on a setup → build → push → deploy workflow with optional registry-based image distribution.

## Tech stack

- **Language:** Rust (edition 2024)
- **CLI parsing:** clap (derive)
- **SSH:** ssh2 crate
- **Serialization:** serde + serde_json, serde_yaml
- **TUI:** ratatui (wired but not yet used in main flow)
- **Tab completion:** rustyline (SSH key path input only)
- **Error handling:** anyhow throughout

## Project structure

```
justdeploy/
├── README.md
├── agents.md                          # This file
└── cli/
    ├── Cargo.toml
    └── src/
        ├── main.rs                    # Entry point — matches CLI commands to handlers
        ├── cli.rs                     # Clap command/arg definitions
        ├── commands/
        │   ├── mod.rs
        │   ├── setup.rs               # yolped setup / setup server / setup registry
        │   ├── build.rs               # (handled inline in main.rs)
        │   ├── push.rs                # yolped push — build + push to registry
        │   ├── up.rs                  # yolped up — build + push + deploy in one step
        │   ├── deploy.rs              # yolped deploy — run on server (local or remote)
        │   ├── logs.rs                # yolped logs
        │   ├── list.rs                # yolped list
        │   └── ssh.rs                 # yolped ssh
        └── core/
            ├── mod.rs
            ├── logger.rs              # Coloured terminal output (info/warn/success)
            ├── runner.rs              # Local docker build / run / stop / logs
            ├── remote_runner.rs       # Remote docker commands over SSH
            ├── ssh.rs                 # SshConnection wrapper (exec_stream, upload, etc.)
            ├── registry.rs            # ~/.yolped/registry.json tracker
            ├── utils.rs               # generate_deployment_name()
            ├── app.rs                 # TUI App state
            ├── tui.rs                 # Terminal lifecycle
            ├── event.rs               # Crossterm event loop (MPSC)
            ├── update.rs              # Key event → App state
            └── models/
                ├── mod.rs
                ├── config.rs          # JdConfig and all sub-structs (source of truth)
                └── deployment.rs      # Deployment struct + parse_file()
```

## Config schema (`yolped.json`)

Defined in `core/models/config.rs`. All commands read the file fresh on every run.

```json
{
  "name": "myapp",
  "version": "1.0.0",
  "build": {
    "files": [
      {
        "file": "services/api/Dockerfile",
        "image": "ghcr.io/user/myapp-api",
        "build_args": { "NODE_VERSION": "20" }
      },
      {
        "file": "services/web/Dockerfile",
        "image": "ghcr.io/user/myapp-web"
      }
    ],
    "platform": "linux/amd64"
  },
  "deploy": {
    "file": "docker-compose.yml",
    "args": ["--remove-orphans"]
  },
  "registry": {
    "image": "ghcr.io/user/myapp",
    "tags": ["latest", "@version"]
  },
  "server": {
    "host": "203.0.113.10",
    "user": "ubuntu",
    "auth": { "Key": "/Users/you/.ssh/id_rsa" },
    "remote_dir": "~/deployments"
  }
}
```

### Struct map

| Struct | Fields | Purpose |
|---|---|---|
| `JdConfig` | `name`, `version`, `build`, `deploy`, `registry?`, `server?` | Top-level config |
| `BuildConfig` | `files[]`, `platform?` | What to build and for which platform |
| `BuildFile` | `file`, `image?`, `build_args{}` | One Dockerfile entry in the build pipeline |
| `DeployConfig` | `file`, `args[]` | What to run on the server and with what flags |
| `RegistryConfig` | `image?`, `tags[]` | Registry push target and tag list |
| `ServerConfig` | `host`, `user`, `auth`, `remote_dir` | Remote SSH server |
| `SshAuth` | `Password` \| `Key(PathBuf)` | SSH auth method |

`registry` and `server` are both `Option<_>` — omit for local-only or no-registry projects.

## Commands

| Command | Handler | Description |
|---|---|---|
| `yolped setup` | `commands/setup.rs::run()` | Interactive wizard: name, detect docker file, server, detect remote platform |
| `yolped setup server` | `commands/setup.rs::run_server()` | Reconfigure only the server section |
| `yolped setup registry` | `commands/setup.rs::run_registry()` | Configure image/tags for push |
| `yolped build` | `main.rs` inline | Build all `build.files` (or `deploy.file` fallback) with buildx if platform set |
| `yolped push [--tag T] [--version V]` | `commands/push.rs::run()` | Build + push each file with resolved tags |
| `yolped up [--tag T] [--version V]` | `commands/up.rs::run()` | build → push → deploy in sequence |
| `yolped deploy [--down] [--rebuild] [--local]` | `commands/deploy.rs::run()` | Deploy to server; pulls from registry if configured, else builds on remote |
| `yolped logs [name]` | `commands/logs.rs::run()` | Stream logs from local or remote container |
| `yolped list` | `commands/list.rs::run()` | List all registered deployments with live status |
| `yolped ssh` | `commands/ssh.rs::run()` | Open interactive SSH session to configured remote |

## Versioned tags

`registry.tags` supports an `@version` placeholder resolved at push time from `JdConfig::version`:

- Config: `"tags": ["latest", "@version"]`, `"version": "1.2.3"` → pushes `latest` and `1.2.3`
- `yolped push --version 2.0.0` overrides the version for that run and adds it as an extra tag
- `yolped up --version 2.0.0` does the same across the full build → push → deploy pipeline

## Remote deploy flow

### Registry-first (when `registry` is configured)
1. `yolped push` builds locally with `docker buildx build --load --platform <platform>` and pushes all tags
2. `yolped deploy` SSHs to server, runs `docker pull <image>:<tag>` + `docker run` (Dockerfile) or `docker compose pull` + `docker compose up -d` (Compose)
3. Server needs no build tools — only Docker

### Build-on-remote (no registry)
1. `yolped deploy` SSHs to server, uploads the Dockerfile or compose file, syncs env files
2. Runs `docker build` on the remote using the detected `build.platform`
3. Starts containers, streams output live

## Key implementation details

### `core/ssh.rs` — `SshConnection`
- `connect(&ServerConfig)` — TCP + SSH handshake + auth (password via rpassword, key via ssh2)
- `exec_stream(&str)` — non-blocking poll of stdout+stderr, streams live to terminal, returns exit code
- `exec_output(&str)` — captures stdout as String (silent)
- `upload(&Path, &str)` — SCP upload of a single file
- `expand_path(&str)` — resolves `~` to remote home dir
- `mkdir_p(&str)`, `file_exists(&str)`, `image_exists(&str)` — remote helpers

### `core/remote_runner.rs`
- All shell interpolation goes through `shell_escape()` — wraps in single quotes, escapes `'` as `'\''`
- `detect_platform(&SshConnection)` — runs `uname -m`, maps to Docker platform string
- `pull_from_registry()` — pulls image on remote; for Dockerfile projects, tags it with `deployment.name` so the deploy step can reference it by name
- `parse_env_files()` — parses `env_file:` entries from compose YAML; rejects paths outside the project dir (path traversal guard)
- `build()`, `deploy()`, `stop()`, `logs()`, `remove_images()` — remote Docker commands

### `core/runner.rs`
- `build(&Deployment, platform: Option<&str>)` — uses `docker buildx build --load` when platform is set, else `docker build`; for compose uses `DOCKER_DEFAULT_PLATFORM` env var
- `build_file(&Path, local_name, platform, build_args)` — builds a single Dockerfile with explicit name and `--build-arg`s
- `deploy()`, `stop()`, `logs()`, `check_status()`, `image_exists()` — local Docker commands

### `commands/setup.rs`
- SSH key path prompt uses **rustyline** with `FilenameCompleter` for Tab autocomplete
- After configuring a remote server, immediately SSHes in to detect the CPU architecture and stores it as `build.platform`
- Name validation: `[a-zA-Z0-9._-]` only — safe for Docker container names and shell commands

### `commands/push.rs`
- When `build.files` is non-empty: builds each file in sequence with its own image + build_args, then pushes all resolved tags
- When `build.files` is empty: falls back to `deploy.file` for backward compat
- Tags are resolved before build: `@version` → actual version string; `--version` override applies to all tags
- Uses `docker buildx build --load` with `--platform` when `build.platform` is set

### `core/registry.rs` — `~/.yolped/registry.json`
- Tracks registered projects (path to `yolped.json`, last_built_at, last_deployed_at, status)
- `yolped list` does a live `docker inspect` / `docker compose ps` check on every entry

## Security notes (already applied)

- Shell injection: all values interpolated into remote shell commands are wrapped via `shell_escape()` — no raw string interpolation
- Path traversal: `env_file` entries outside the project dir are rejected with a hard bail (no fallback)
- Deployment names validated to `[a-zA-Z0-9._-]` only — prevents injection via the name field
- SSH passwords are prompted at runtime via `rpassword` — never stored anywhere
- SSH keys stored as absolute paths in config, never transmitted

## Branch

Current active branch: `remote-deploy`. Merge target: `main`.
