use crate::LlamaClient;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
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

pub struct LlamaSidecar {
    child: Child,
    pub base_url: String,
    pub api_key: String,
}

impl LlamaSidecar {
    pub async fn spawn(config: SidecarConfig) -> Result<Self, SidecarError> {
        let port = free_port()?;
        let api_key = format!("palor-{}", Uuid::new_v4());
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
        let mut sidecar = Self {
            child: command.spawn()?,
            base_url: format!("http://127.0.0.1:{port}"),
            api_key,
        };
        let client = LlamaClient::new(&sidecar.base_url, &sidecar.api_key)?;
        for _ in 0..120 {
            if let Some(status) = sidecar.child.try_wait()? {
                return Err(SidecarError::EarlyExit(status.to_string()));
            }
            if client.health().await.is_ok() {
                return Ok(sidecar);
            }
            sleep(Duration::from_millis(250)).await;
        }
        sidecar.stop().await?;
        Err(SidecarError::HealthTimeout)
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
    #[error("llama.cpp exited before becoming healthy: {0}")]
    EarlyExit(String),
    #[error("llama.cpp did not become healthy within 30 seconds")]
    HealthTimeout,
}
