use crate::verify_file;
use futures_util::StreamExt;
use reqwest::{header, Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::{io::AsyncWriteExt, sync::watch};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSpec {
    pub id: String,
    pub url: String,
    pub destination: PathBuf,
    pub expected_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub bytes_per_second: u64,
    pub verifying: bool,
}

#[derive(Clone)]
pub struct Downloader {
    client: Client,
}

impl Downloader {
    pub fn new() -> Result<Self, DownloadError> {
        let client = Client::builder()
            .user_agent("Veda/0.1 (+https://github.com/Katsugachi/Project-Palor)")
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(60 * 60))
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()?;
        Ok(Self { client })
    }

    pub async fn download(
        &self,
        spec: &DownloadSpec,
        progress: watch::Sender<DownloadProgress>,
    ) -> Result<(), DownloadError> {
        ensure_parent(&spec.destination).await?;
        let part_path = part_path(&spec.destination);
        let mut offset = tokio::fs::metadata(&part_path)
            .await
            .map_or(0, |metadata| metadata.len());
        if offset > spec.expected_bytes {
            tokio::fs::remove_file(&part_path).await?;
            offset = 0;
        }

        let mut request = self.client.get(&spec.url);
        if offset > 0 {
            request = request.header(header::RANGE, format!("bytes={offset}-"));
        }
        let mut response = request.send().await?;
        if offset > 0 && response.status() == StatusCode::OK {
            tokio::fs::remove_file(&part_path).await?;
            offset = 0;
            response = self.client.get(&spec.url).send().await?;
        }
        if !(response.status().is_success() || response.status() == StatusCode::PARTIAL_CONTENT) {
            return Err(DownloadError::HttpStatus(response.status()));
        }

        let mut options = tokio::fs::OpenOptions::new();
        options.create(true).write(true);
        if offset > 0 {
            options.append(true);
        } else {
            options.truncate(true);
        }
        let mut file = options.open(&part_path).await?;
        let started = Instant::now();
        let start_bytes = offset;
        let mut downloaded = offset;
        let mut last_report = Instant::now();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            if downloaded > spec.expected_bytes {
                return Err(DownloadError::TooLarge {
                    expected: spec.expected_bytes,
                    actual: downloaded,
                });
            }
            if last_report.elapsed() >= Duration::from_millis(100) {
                let elapsed = started.elapsed().as_secs_f64().max(0.001);
                let speed = ((downloaded - start_bytes) as f64 / elapsed) as u64;
                let _ = progress.send(DownloadProgress {
                    id: spec.id.clone(),
                    downloaded_bytes: downloaded,
                    total_bytes: spec.expected_bytes,
                    bytes_per_second: speed,
                    verifying: false,
                });
                last_report = Instant::now();
            }
        }
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        let _ = progress.send(DownloadProgress {
            id: spec.id.clone(),
            downloaded_bytes: downloaded,
            total_bytes: spec.expected_bytes,
            bytes_per_second: 0,
            verifying: true,
        });
        verify_file(&part_path, spec.expected_bytes, &spec.sha256).await?;
        replace_atomically(&part_path, &spec.destination).await?;
        Ok(())
    }
}

async fn ensure_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    Ok(())
}

fn part_path(destination: &Path) -> PathBuf {
    let mut value = destination.as_os_str().to_os_string();
    value.push(".part");
    PathBuf::from(value)
}

async fn replace_atomically(from: &Path, to: &Path) -> std::io::Result<()> {
    if tokio::fs::try_exists(to).await? {
        tokio::fs::remove_file(to).await?;
    }
    tokio::fs::rename(from, to).await
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Verify(#[from] crate::verify::VerifyError),
    #[error("download server returned {0}")]
    HttpStatus(StatusCode),
    #[error("download exceeded expected size: expected {expected}, got {actual}")]
    TooLarge { expected: u64, actual: u64 },
    #[error("download was cancelled")]
    Cancelled,
}
