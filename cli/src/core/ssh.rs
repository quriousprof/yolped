use std::{
    io::{Read, Write},
    net::TcpStream,
    path::Path,
};

use anyhow::{bail, Context, Result};
use ssh2::Session;

use super::models::deployment::{RemoteServer, SshAuth};

pub struct SshConnection {
    session: Session,
}

impl SshConnection {
    pub fn connect(server: &RemoteServer) -> Result<Self> {
        let addr = format!("{}:22", server.ip);
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

        Ok(Self { session })
    }

    /// Execute a command and stream its stdout to the terminal live.
    /// Returns the exit code.
    pub fn exec_stream(&self, cmd: &str) -> Result<i32> {
        let mut channel = self
            .session
            .channel_session()
            .context("Failed to open SSH channel")?;
        channel
            .exec(cmd)
            .with_context(|| format!("Failed to exec: {}", cmd))?;

        // Stream stdout live
        let mut buf = [0u8; 4096];
        loop {
            match channel.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    std::io::stdout().write_all(&buf[..n])?;
                    std::io::stdout().flush()?;
                }
                Err(e) => return Err(e.into()),
            }
        }

        // Drain stderr after stdout is done
        {
            let mut stderr_buf = Vec::new();
            channel.stderr().read_to_end(&mut stderr_buf)?;
            if !stderr_buf.is_empty() {
                std::io::stderr().write_all(&stderr_buf)?;
            }
        }

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
