mod downloader;
mod verify;

pub use downloader::{DownloadError, DownloadProgress, DownloadSpec, Downloader};
pub use verify::{sha256_file, verify_file};
