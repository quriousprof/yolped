# yolped

A CLI tool for managing deployments directly from your terminal.

## What it does

yolped lets you deploy projects to remote servers without leaving the terminal. The core workflow:

1. **Add a project** — point yolped at a directory. It uses a `yolped.json` config file to understand how to build and deploy, or you can provide a Dockerfile directly.
2. **Add servers** — register server IPs with password or SSH key authentication.
3. **Deploy** — yolped SSHs into the target server(s) and handles the deployment.

## Tech stack

- **Language:** Rust
- **TUI:** ratatui (terminal UI framework)
- **CLI location:** `cli/` directory

## Project structure

```
yolped/
├── cli/                        # Rust CLI application
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs             # Entry point — wires CLI → Deployment → runner
│       ├── cli.rs              # Clap argument struct (Cli)
│       ├── app.rs              # TUI application state (App)
│       ├── tui.rs              # Terminal lifecycle (enter/exit/draw)
│       ├── event.rs            # Crossterm event handler (threaded, MPSC)
│       ├── update.rs           # Key-event → App state dispatcher
│       ├── utils.rs            # Shared utilities (e.g. generate_deployment_name)
│       ├── runner.rs           # Build execution (docker build / docker compose)
│       ├── models/
│       │   ├── mod.rs
│       │   └── deployment.rs   # Deployment struct, enums, parse_file()
│       └── ui/
│           ├── mod.rs
│           └── render.rs       # Ratatui render function
└── agents.md                   # This file
```

## Module responsibilities

| Module | Responsibility |
|---|---|
| `cli.rs` | CLI argument definitions via Clap |
| `models/deployment.rs` | `Deployment` data model, `DeploymentType`/`ServerType`/`DeploymentStatus` enums, `parse_file()` |
| `runner.rs` | Executes the actual build (currently: `docker build`; stub for docker-compose) |
| `utils.rs` | Stateless helpers (random name generation) |
| `app.rs` | TUI `App` state struct |
| `tui.rs` | Terminal setup/teardown and draw loop |
| `event.rs` | Threaded crossterm event polling over MPSC channel |
| `update.rs` | Maps key events to `App` state mutations |
| `ui/render.rs` | Ratatui frame rendering |

## Key concepts

- **`yolped.json`** — per-project config file that lives in the project directory. Defines how the project should be built and deployed.
- **Dockerfile support** — projects can alternatively provide a Dockerfile instead of `yolped.json` for containerised deployments.
- **Server management** — users register remote servers (IP + password or SSH key) that yolped can deploy to.
- **SSH-based deployment** — deployments happen over SSH to the registered servers.

## Error handling

All fallible operations use `color_eyre::eyre::Result`. `Deployment::new()` returns `Result<Self>` so file-parsing errors propagate cleanly to `main`. `runner::build()` returns `Result<()>` and checks the docker process exit code.

## Status

Early stage. Implemented so far:
- CLI argument parsing (`--file`, optional `name`)
- File-type detection (`Dockerfile` / docker-compose YAML)
- `docker build` execution with inherited stdio and exit-code checking
- TUI infrastructure (terminal lifecycle, event loop, render skeleton) — ready but not yet wired into the main flow
