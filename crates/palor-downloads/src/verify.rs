use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::io::AsyncReadExt;

pub async fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub async fn verify_file(
    path: &Path,
    expected_bytes: u64,
    expected_sha256: &str,
) -> Result<(), VerifyError> {
    let metadata = tokio::fs::metadata(path).await?;
    if metadata.len() != expected_bytes {
        return Err(VerifyError::Size {
            expected: expected_bytes,
            actual: metadata.len(),
        });
    }
    let actual = sha256_file(path).await?;
    if !actual.eq_ignore_ascii_case(expected_sha256) {
        return Err(VerifyError::Checksum {
            expected: expected_sha256.into(),
            actual,
        });
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("file size mismatch: expected {expected} bytes, got {actual}")]
    Size { expected: u64, actual: u64 },
    #[error("SHA-256 mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn verifies_known_content() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("asset.bin");
        tokio::fs::write(&file, b"palor").await.unwrap();
        verify_file(
            &file,
            5,
            "039f23b88724002ebef0771882a0ef93f43e4a0d969bd4010d3416bb348f8af7",
        )
        .await
        .unwrap();
    }
}
