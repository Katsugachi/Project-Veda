use crate::session::SessionHandle;
use parking_lot::RwLock;
use std::{path::PathBuf, sync::Arc};
use tauri::{AppHandle, Manager};

pub struct AppState {
    pub data_dir: PathBuf,
    pub downloads: Arc<RwLock<Vec<crate::commands::DownloadItem>>>,
    pub search_index: Arc<RwLock<Option<Arc<veda_search::HybridIndex>>>>,
    /// The warm model runtime. The chat (and, once needed, embedding) llama.cpp
    /// sidecar stays loaded between requests so a follow-up question does not
    /// re-read a ~750 MB model off disk — the dominant cause of the old
    /// "every question takes minutes" behaviour.
    pub runtime: SessionHandle,
}

impl AppState {
    pub fn new(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let data_dir = app.path().app_local_data_dir()?;
        std::fs::create_dir_all(&data_dir)?;
        for directory in [
            "models",
            "runtime",
            "docsets",
            "indexes",
            "downloads",
            "logs",
        ] {
            std::fs::create_dir_all(data_dir.join(directory))?;
        }
        Ok(Self {
            data_dir,
            downloads: Arc::new(RwLock::new(Vec::new())),
            search_index: Arc::new(RwLock::new(None)),
            runtime: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }
}
