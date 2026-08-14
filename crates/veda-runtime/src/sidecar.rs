use crate::LlamaClient;
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    time::sleep,
};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SidecarConfig {
    pub executable: PathBuf,
    pub model: PathBuf,
    pub context_tokens: u32,
    pub gpu_layers: i32,
    pub embedding: bool,
    pub pooling: Option<String>,
    pub threads: usize,
}

/// Ring buffer that keeps the most recent llama.cpp log lines. Logs are only
/// read back when startup fails, so the process can write as much output as
/// it wants without ever blocking on a full pipe buffer.
#[derive(Default)]
struct LogTail {
    bytes: Mutex<Vec<u8>>,
}

impl LogTail {
    const MAX_BYTES: usize = 16 * 1024;

    fn push(&self, chunk: &[u8]) {
        let mut bytes = self
            .bytes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        bytes.extend_from_slice(chunk);
        if bytes.len() > Self::MAX_BYTES {
            let excess = bytes.len() - Self::MAX_BYTES;
            bytes.drain(..excess);
        }
    }

    fn snapshot(&self) -> String {
        let bytes = self
            .bytes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let mut lines = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>();
        if lines.len() > 6 {
            lines.drain(..lines.len() - 6);
        }
        lines.join("\n")
    }
}

async fn drain_log<R>(reader: R, tail: Arc<LogTail>)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        tail.push(line.as_bytes());
        tail.push(b"\n");
    }
}

pub struct LlamaSidecar {
    child: Child,
    pub base_url: String,
    pub api_key: String,
    logs: Arc<LogTail>,
}

impl LlamaSidecar {
    pub async fn spawn(config: SidecarConfig) -> Result<Self, SidecarError> {
        // Two chats can ask at once and each spawns its own llama.cpp
        // sidecar. The OS hands out the probe port before the child binds it,
        // so a collision between two concurrent spawns is possible; retry on
        // a fresh port a couple of times before surfacing the error.
        let mut attempts = 0_u32;
        loop {
            match spawn_once(config.clone()).await {
                Ok(sidecar) => return Ok(sidecar),
                Err(error) if attempts < 2 && is_bind_collision(&error) => {
                    attempts += 1;
                    tracing::warn!(
                        %error,
                        attempts,
                        "llama.cpp port collision; retrying on a new port"
                    );
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// True when llama.cpp failed because another process (usually a second
/// sidecar spawned concurrently) already owns the chosen port. Detected from
/// the captured log tail so the retry only fires for actual collisions.
fn is_bind_collision(error: &SidecarError) -> bool {
    let SidecarError::EarlyExit { log, .. } = error else {
        return false;
    };
    let log = log.to_ascii_lowercase();
    log.contains("address already in use") || log.contains("wsaeaddrinuse") || log.contains("10048")
}

impl LlamaSidecar {
    async fn spawn_once(config: SidecarConfig) -> Result<Self, SidecarError> {
        let port = free_port()?;
        let api_key = format!("veda-{}", Uuid::new_v4());
        let mut command = Command::new(&config.executable);
        command
            .arg("--model")
            .arg(&config.model)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--ctx-size")
            .arg(config.context_tokens.to_string())
            .arg("--threads")
            .arg(config.threads.max(1).to_string())
            .arg("--n-gpu-layers")
            .arg(config.gpu_layers.to_string())
            .arg("--api-key")
            .arg(&api_key)
            .arg("--jinja")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
        }
        if config.embedding {
            command
                .arg("--embeddings")
                .arg("--batch-size")
                .arg("2048")
                .arg("--ubatch-size")
                .arg("512");
            if let Some(pooling) = &config.pooling {
                command.arg("--pooling").arg(pooling);
            }
        }
        let logs = Arc::new(LogTail::default());
        let mut sidecar = Self {
            child: command.spawn()?,
            base_url: format!("http://127.0.0.1:{port}"),
            api_key,
            logs: Arc::clone(&logs),
        };
        // Drain both pipes so a chatty llama.cpp build can never fill the
        // OS pipe buffer and stall the server.
        if let Some(stdout) = sidecar.child.stdout.take() {
            tokio::spawn(drain_log(stdout, Arc::clone(&logs)));
        }
        if let Some(stderr) = sidecar.child.stderr.take() {
            tokio::spawn(drain_log(stderr, logs));
        }
        let client = LlamaClient::new(&sidecar.base_url, &sidecar.api_key)?;
        for _ in 0..120 {
            if let Some(status) = sidecar.child.try_wait()? {
                return Err(SidecarError::EarlyExit {
                    status: status.to_string(),
                    log: SidecarError::log_suffix(&sidecar.logs.snapshot()),
                });
            }
            if client.health().await.is_ok() {
                return Ok(sidecar);
            }
            sleep(Duration::from_millis(250)).await;
        }
        let log = SidecarError::log_suffix(&sidecar.logs.snapshot());
        sidecar.stop().await?;
        Err(SidecarError::HealthTimeout { log })
    }

    pub async fn stop(&mut self) -> Result<(), std::io::Error> {
        if self.child.try_wait()?.is_none() {
            self.child.kill().await?;
        }
        Ok(())
    }
}

impl Drop for LlamaSidecar {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    Ok(listener.local_addr()?.port())
}

#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Client(#[from] crate::LlamaClientError),
    #[error("llama.cpp exited before becoming healthy: {status}{log}")]
    EarlyExit { status: String, log: String },
    #[error("llama.cpp did not become healthy within 30 seconds{log}")]
    HealthTimeout { log: String },
}

impl SidecarError {
    /// Formats the captured log tail as a display suffix, or an empty string
    /// when llama.cpp produced no output.
    fn log_suffix(log: &str) -> String {
        if log.is_empty() {
            String::new()
        } else {
            format!("\nllama.cpp output:\n{log}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_collision_is_detected_from_the_log_tail() {
        let posix = SidecarError::EarlyExit {
            status: "exit status: 1".into(),
            log: "llama_server: error: bind() failed: Address already in use".into(),
        };
        assert!(is_bind_collision(&posix));

        let winsock = SidecarError::EarlyExit {
            status: "exit status: 1".into(),
            log: "llama_server: error: bind() failed with error 10048".into(),
        };
        assert!(is_bind_collision(&winsock));

        let unrelated = SidecarError::EarlyExit {
            status: "exit status: 1".into(),
            log: "llama_model_load: error loading model".into(),
        };
        assert!(!is_bind_collision(&unrelated));

        // A bind failure that outlived the retry window is a health timeout,
        // which must not be retried a second time by the caller's loop.
        let timeout = SidecarError::HealthTimeout {
            log: "llama.cpp output:\nbind() failed: Address already in use".into(),
        };
        assert!(!is_bind_collision(&timeout));
    }
}
