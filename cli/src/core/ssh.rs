use std::{
    io::{self, ErrorKind, Read, Write},
    net::TcpStream,
    path::Path,
    thread,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use ssh2::Session;

use super::{logger, models::deployment::{RemoteServer, SshAuth}};

pub struct SshConnection {
    session: Session,
}

impl SshConnection {
    pub fn connect(server: &RemoteServer) -> Result<Self> {
        let addr = format!("{}:22", server.ip);
        logger::info(&format!("Connecting to {}@{}...", server.user, server.ip));

        let tcp = TcpStream::connect(&addr)
            .with_context(|| format!("Failed to connect to {}", addr))?;

        let mut session = Session::new().context("Failed to create SSH session")?;
        session.set_tcp_stream(tcp);
        session.handshake().context("SSH handshake failed")?;

        match &server.auth {
            SshAuth::Password => {
                let password = rpassword::prompt_password(format!(
                    "Password for {}@{}: ",
                    server.user, server.ip
                ))?;
                session
                    .userauth_password(&server.user, &password)
                    .context("Password authentication failed")?;
            }
            SshAuth::Key(key_path) => {
                session
                    .userauth_pubkey_file(&server.user, None, key_path, None)
                    .context("SSH key authentication failed")?;
            }
        }

        if !session.authenticated() {
            bail!("Authentication failed for {}@{}", server.user, server.ip);
        }

        logger::success(&format!("Connected to {}@{}", server.user, server.ip));
        Ok(Self { session })
    }

    /// Execute a command and stream stdout + stderr to the terminal in real-time.
    /// Returns the exit code.
    pub fn exec_stream(&self, cmd: &str) -> Result<i32> {
        let mut channel = self
            .session
            .channel_session()
            .context("Failed to open SSH channel")?;
        channel
            .exec(cmd)
            .with_context(|| format!("Failed to exec: {}", cmd))?;

        // Switch to non-blocking so we can poll both stdout and stderr
        // interleaved, giving real-time output.
        self.session.set_blocking(false);

        let mut buf = [0u8; 4096];
        let mut sbuf = [0u8; 4096];

        loop {
            let mut activity = false;

            // --- stdout ---
            match channel.read(&mut buf) {
                Ok(0) => {}
                Ok(n) => {
                    io::stdout().write_all(&buf[..n])?;
                    io::stdout().flush()?;
                    activity = true;
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => {
                    self.session.set_blocking(true);
                    return Err(e.into());
                }
            }

            // --- stderr ---
            {
                let mut stderr = channel.stderr();
                match stderr.read(&mut sbuf) {
                    Ok(0) => {}
                    Ok(n) => {
                        io::stderr().write_all(&sbuf[..n])?;
                        io::stderr().flush()?;
                        activity = true;
                    }
                    Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(e) => {
                        self.session.set_blocking(true);
                        return Err(e.into());
                    }
                }
            }

            // EOF from remote + no pending data → done
            if channel.eof() && !activity {
                break;
            }

            // Yield briefly when idle to avoid burning CPU
            if !activity {
                thread::sleep(Duration::from_millis(10));
            }
        }

        self.session.set_blocking(true);
        channel.wait_close().context("Failed to wait for channel close")?;
        Ok(channel.exit_status()?)
    }

    /// Check whether a Docker image exists on the remote host.
    pub fn image_exists(&self, name: &str) -> bool {
        self.exec_stream(&format!(
            "docker image inspect {} > /dev/null 2>&1",
            name
        ))
        .map(|code| code == 0)
        .unwrap_or(false)
    }

    /// Capture stdout of a command as a string (does not print to terminal).
    pub fn exec_output(&self, cmd: &str) -> Result<String> {
        let mut channel = self
            .session
            .channel_session()
            .context("Failed to open SSH channel")?;
        channel.exec(cmd).with_context(|| format!("Failed to exec: {}", cmd))?;

        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        channel.wait_close()?;
        Ok(output.trim().to_string())
    }

    /// Expand a leading `~` to the remote user's home directory.
    pub fn expand_path(&self, path: &str) -> Result<String> {
        if !path.starts_with('~') {
            return Ok(path.to_string());
        }
        let home = self.exec_output("echo $HOME")?;
        Ok(path.replacen('~', &home, 1))
    }

    /// Return true if a file exists at the given remote path.
    pub fn file_exists(&self, path: &str) -> bool {
        self.exec_output(&format!("test -f '{}' && echo 1 || echo 0", path))
            .map(|s| s.trim() == "1")
            .unwrap_or(false)
    }

    /// Create a directory (and parents) on the remote.
    pub fn mkdir_p(&self, path: &str) -> Result<()> {
        let code = self.exec_stream(&format!("mkdir -p '{}'", path))?;
        if code != 0 {
            bail!("Failed to create remote directory '{}'", path);
        }
        Ok(())
    }

    /// Upload a single local file to a remote path via SCP.
    pub fn upload(&self, local: &Path, remote_path: &str) -> Result<()> {
        let contents = std::fs::read(local)
            .with_context(|| format!("Failed to read '{}'", local.display()))?;

        let mut remote_file = self
            .session
            .scp_send(Path::new(remote_path), 0o644, contents.len() as u64, None)
            .with_context(|| format!("Failed to open SCP channel for '{}'", remote_path))?;

        remote_file.write_all(&contents)?;
        remote_file.send_eof()?;
        remote_file.wait_eof()?;
        remote_file.close()?;
        remote_file.wait_close()?;

        Ok(())
    }
}
