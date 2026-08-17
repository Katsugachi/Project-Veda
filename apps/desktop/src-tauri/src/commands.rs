use crate::{
    session::{ModelSession, SessionFingerprint},
    state::AppState,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use sysinfo::{Disks, System};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use veda_core::{
    contextual_error, default_catalog, is_conversational, AskRequest, AskResponse, Catalog,
    HardwareSnapshot, ModelQuant, PreflightReport, SourceRef, CONVERSATIONAL_SYSTEM_PROMPT,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Docset {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub version: String,
    pub compressed_bytes: u64,
    pub installed_bytes: u64,
    pub state: String,
    pub progress: f32,
    pub pages: Option<usize>,
    pub accent: String,
    pub initials: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderSource {
    pub title: String,
    pub section: String,
    pub docset: String,
    pub text: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub state: String,
    pub progress: f32,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub speed_bytes: Option<u64>,
    /// The docset this download belongs to (source archives and search
    /// indexes), so the UI can mirror live progress on the matching doc card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docset: Option<String>,
}

#[tauri::command]
pub async fn system_preflight(state: State<'_, AppState>) -> Result<PreflightReport, String> {
    let mut system = System::new_all();
    system.refresh_all();
    let disks = Disks::new_with_refreshed_list();
    let disk = disks
        .iter()
        .filter(|disk| state.data_dir.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .or_else(|| disks.iter().max_by_key(|disk| disk.available_space()));
    let (free_disk_bytes, disk_kind) =
        disk.map_or((0, veda_core::preflight::DiskKind::Unknown), |disk| {
            let kind = match disk.kind() {
                sysinfo::DiskKind::SSD => veda_core::preflight::DiskKind::Ssd,
                sysinfo::DiskKind::HDD => veda_core::preflight::DiskKind::Hdd,
                _ => veda_core::preflight::DiskKind::Unknown,
            };
            (disk.available_space(), kind)
        });
    Ok(veda_core::evaluate_preflight(HardwareSnapshot {
        total_memory_bytes: system.total_memory(),
        available_memory_bytes: system.available_memory(),
        free_disk_bytes,
        disk_kind,
        architecture: std::env::consts::ARCH.into(),
        operating_system: format!(
            "{} {}",
            System::name().unwrap_or_else(|| std::env::consts::OS.into()),
            System::os_version().unwrap_or_default()
        )
        .trim()
        .into(),
    }))
}

#[tauri::command]
pub fn resource_catalog() -> Catalog {
    default_catalog()
}

#[tauri::command]
pub async fn prepare_resources(
    quant: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let quant = match quant.as_str() {
        "q5" => veda_core::ModelQuant::Q5,
        "q8" => veda_core::ModelQuant::Q8,
        _ => return Err(format!("unsupported model quantization: {quant}")),
    };
    crate::resources::prepare(app, &state, quant).await
}

#[tauri::command]
pub fn list_docsets(state: State<'_, AppState>) -> Vec<Docset> {
    docsets()
        .into_iter()
        .map(|mut docset| {
            let manifest_path = state
                .data_dir
                .join("docsets")
                .join(&docset.id)
                .join("manifest.json");
            let index_path = state
                .data_dir
                .join("indexes")
                .join(format!("{}.json.zst", docset.id));
            if manifest_path.exists() && index_path.exists() {
                let manifest = std::fs::read(&manifest_path).ok().and_then(|bytes| {
                    serde_json::from_slice::<veda_docs::DocPackManifest>(&bytes).ok()
                });
                if let Some(manifest) = manifest {
                    docset.pages = Some(manifest.page_count);
                    docset.state = if crate::resources::expected_docset_version(&docset.id)
                        == Some(manifest.version.as_str())
                    {
                        "installed"
                    } else {
                        "updateAvailable"
                    }
                    .into();
                    docset.progress = 100.0;
                }
            }
            docset
        })
        .collect()
}

#[tauri::command]
pub fn list_downloads(state: State<'_, AppState>) -> Vec<DownloadItem> {
    let mut items = state.downloads.read().clone();
    for asset in default_catalog().assets {
        let filename = asset.url.rsplit('/').next().unwrap_or(&asset.id);
        let location = match asset.kind {
            veda_core::AssetKind::ChatModel | veda_core::AssetKind::EmbeddingModel => {
                state.data_dir.join("models").join(filename)
            }
            veda_core::AssetKind::LlamaRuntime | veda_core::AssetKind::RuntimeDependency => {
                state.data_dir.join("runtime").join(&asset.id)
            }
            veda_core::AssetKind::Docset => continue,
        };
        if location.exists() && !items.iter().any(|item| item.id == asset.id) {
            items.push(DownloadItem {
                id: asset.id,
                name: asset.name,
                detail: String::new(),
                state: "installed".into(),
                progress: 100.0,
                downloaded_bytes: asset.bytes,
                total_bytes: asset.bytes,
                speed_bytes: None,
                docset: None,
            });
        }
    }
    items
}

#[tauri::command]
pub async fn install_docset(
    id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let id = match id.as_str() {
        "python" => veda_core::DocsetId::Python,
        "cpp" => veda_core::DocsetId::Cpp,
        "html" => veda_core::DocsetId::Html,
        "css" => veda_core::DocsetId::Css,
        "javascript" => veda_core::DocsetId::Javascript,
        _ => return Err(format!("unknown documentation pack: {id}")),
    };
    crate::resources::install_docset(app, &state, id).await
}

#[tauri::command]
pub async fn remove_docset(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let id = match id.as_str() {
        "python" => veda_core::DocsetId::Python,
        "cpp" => veda_core::DocsetId::Cpp,
        "html" => veda_core::DocsetId::Html,
        "css" => veda_core::DocsetId::Css,
        "javascript" => veda_core::DocsetId::Javascript,
        _ => return Err(format!("unknown documentation pack: {id}")),
    };
    crate::resources::remove_docset(&state, id).await
}

#[tauri::command]
pub async fn ask_veda(
    request: AskRequest,
    state: State<'_, AppState>,
) -> Result<AskResponse, String> {
    let started = Instant::now();
    let model = model_files(request.model_quant)
        .iter()
        .map(|file| state.data_dir.join("models").join(file))
        .find(|path| path.exists());
    let Some(model) = model else {
        return Ok(AskResponse {
            message_id: uuid::Uuid::new_v4().to_string(),
            content: "MiniCPM 5 is not installed yet. Complete the system check and model download before starting an offline documentation chat.".into(),
            sources: Vec::<SourceRef>::new(),
            trace: None,
        });
    };
    let active: ActiveRuntime = serde_json::from_slice(
        &tokio::fs::read(state.data_dir.join("runtime").join("active.json"))
            .await
            .map_err(|_| "No verified llama.cpp runtime is active. Run setup again.".to_string())?,
    )
    .map_err(|error| contextual_error("The active runtime record is unreadable", &error))?;

    let mut system = sysinfo::System::new_all();
    system.refresh_memory();
    let available_memory = system.available_memory();
    let quant = if model
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|name| name.contains("Q8"))
    {
        ModelQuant::Q8
    } else {
        ModelQuant::Q5
    };
    // An explicit context choice from Settings wins; 0/absent means automatic,
    // which sizes the context to the memory that is actually free right now
    // (available RAM, i.e. total minus what is already in use), leaving the
    // 1.4 GB leeway so llama.cpp can never be asked to over-commit.
    let context =
        veda_core::resolve_context_tokens(request.context_tokens, available_memory, quant);
    let threads = system.cpus().len().clamp(1, 16);
    let gpu_layers = if active.backend == "cpu" { 0 } else { 99 };
    let fingerprint = SessionFingerprint {
        model_path: model,
        context_tokens: context,
        gpu_layers,
        backend: active.backend.clone(),
        quant,
    };

    // Small talk (with no attachments) takes the conversational fast path: no
    // search plan, no embedding sidecar, no index load — a greeting is answered
    // by the warm model instead of paying for the whole retrieval pipeline.
    let conversational = request.attachments.is_empty() && is_conversational(&request.message);

    // Reuse the warm session, or rebuild it when the configuration changed.
    let mut slot = state.runtime.lock().await;
    let mut session = match slot.take() {
        Some(existing) if existing.matches(&fingerprint) => Some(existing),
        Some(mut stale) => {
            stale.stop_all().await;
            None
        }
        None => None,
    };
    let mut session = match session.take() {
        Some(session) => session,
        None => ModelSession::spawn_chat(&active.executable, &fingerprint, threads).await?,
    };

    if conversational {
        let content = session
            .llama
            .complete(
                vec![
                    veda_runtime::ChatMessage {
                        role: "system".into(),
                        content: CONVERSATIONAL_SYSTEM_PROMPT.into(),
                    },
                    veda_runtime::ChatMessage {
                        role: "user".into(),
                        content: request.message.clone(),
                    },
                ],
                veda_core::ReasoningMode::Fast,
                256,
                None,
            )
            .await
            .map_err(|error| contextual_error("The local model could not answer", &error))?;
        session.touch();
        *slot = Some(session);
        return Ok(AskResponse {
            message_id: uuid::Uuid::new_v4().to_string(),
            content,
            sources: Vec::new(),
            trace: Some(veda_core::RetrievalTrace {
                queries: Vec::new(),
                lexical_hits: 0,
                semantic_hits: 0,
                elapsed_ms: started.elapsed().as_millis() as u64,
            }),
        });
    }

    let index = load_search_index(&state, &request.docsets).await?;
    if index.is_empty() {
        session.touch();
        *slot = Some(session);
        return Ok(AskResponse {
            message_id: uuid::Uuid::new_v4().to_string(),
            content: "No selected documentation has been indexed yet. Install at least one documentation pack before asking a sourced question.".into(),
            sources: Vec::new(),
            trace: None,
        });
    }
    let embedding_model = state
        .data_dir
        .join("models")
        .join("bge-small-en-v1.5-q8_0.gguf");
    let embed = session
        .ensure_embedding(&active.executable, &embedding_model, threads)
        .await?
        .clone();
    let engine = veda_runtime::VedaEngine::new(
        session.llama.clone(),
        LocalRetriever {
            index,
            embed,
            allowed_docsets: request.docsets.clone(),
        },
    );
    let result = engine
        .ask(request)
        .await
        .map_err(|error| contextual_error("The local model could not answer", &error));
    // Keep the warm session even when this answer failed (a transient model
    // error should not discard ~800 MB of loaded pages), and refresh the idle
    // clock so the janitor does not evict a session that is still in use.
    session.touch();
    *slot = Some(session);
    result
}

/// The on-disk chat model candidates, with the quant the UI is configured
/// for first so the loaded model always matches what Settings reports. When
/// the preferred file is missing the other installed model is used, so a
/// change in the model store never strands an ask.
fn model_files(preferred: Option<veda_core::ModelQuant>) -> [&'static str; 2] {
    match preferred {
        Some(veda_core::ModelQuant::Q8) => ["minicpm5-1b-Q8_0.gguf", "minicpm5-1b-Q5_K_M.gguf"],
        _ => ["minicpm5-1b-Q5_K_M.gguf", "minicpm5-1b-Q8_0.gguf"],
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActiveRuntime {
    backend: String,
    executable: PathBuf,
}

struct LocalRetriever {
    index: Arc<veda_search::HybridIndex>,
    embed: veda_runtime::EmbeddingClient,
    allowed_docsets: Vec<veda_core::DocsetId>,
}

#[async_trait::async_trait]
impl veda_runtime::Retriever for LocalRetriever {
    async fn hybrid_search(
        &self,
        plan: &veda_core::SearchQueryPlan,
    ) -> Result<Vec<veda_runtime::RetrievedChunk>, anyhow::Error> {
        let query_vectors = self.embed.embed_queries(&plan.queries).await?;
        let docsets = if plan.docsets.is_empty() {
            self.allowed_docsets.clone()
        } else {
            plan.docsets
                .iter()
                .copied()
                .filter(|docset| {
                    self.allowed_docsets.is_empty() || self.allowed_docsets.contains(docset)
                })
                .collect()
        };
        let mut merged: HashMap<String, veda_runtime::RetrievedChunk> = HashMap::new();
        for (query, vector) in plan.queries.iter().zip(query_vectors) {
            let hits = self.index.search(
                query,
                &vector,
                &plan.symbols,
                &docsets,
                veda_search::HybridSearchOptions {
                    limit: plan.result_count,
                    candidate_limit: 48,
                    ..Default::default()
                },
            )?;
            for hit in hits {
                let Some(chunk) = self.index.chunk(hit.document) else {
                    continue;
                };
                let candidate = veda_runtime::RetrievedChunk {
                    docset: chunk.docset.as_str().into(),
                    version: chunk.version.clone(),
                    title: chunk.title.clone(),
                    section: chunk.section.clone(),
                    url: chunk.url.clone(),
                    text: chunk.text.clone(),
                    score: hit.score,
                    lexical_score: hit.lexical_score,
                    semantic_score: hit.semantic_score,
                };
                merged
                    .entry(chunk.id.clone())
                    .and_modify(|current| {
                        if candidate.score > current.score {
                            *current = candidate.clone();
                        }
                    })
                    .or_insert(candidate);
            }
        }
        let mut results = merged.into_values().collect::<Vec<_>>();
        results.sort_by(|left, right| right.score.total_cmp(&left.score));
        results.truncate(plan.result_count);
        Ok(results)
    }
}

async fn load_search_index(
    state: &AppState,
    _selected: &[veda_core::DocsetId],
) -> Result<Arc<veda_search::HybridIndex>, String> {
    if let Some(index) = state.search_index.read().clone() {
        return Ok(index);
    }
    let index_dir = state.data_dir.join("indexes");
    let mut paths = Vec::new();
    for docset in veda_core::DocsetId::ALL {
        let path = index_dir.join(format!("{}.json.zst", docset.as_str()));
        if path.exists() {
            paths.push(path);
        }
    }
    let chunks =
        tokio::task::spawn_blocking(move || -> Result<Vec<veda_search::SearchChunk>, String> {
            let mut chunks = Vec::new();
            for path in paths {
                let file = File::open(&path)
                    .map_err(|error| contextual_error("Could not read a search index", &error))?;
                let decoder = zstd::Decoder::new(BufReader::new(file))
                    .map_err(|error| contextual_error("Could not read a search index", &error))?;
                let mut loaded: Vec<veda_search::SearchChunk> = serde_json::from_reader(decoder)
                    .map_err(|error| contextual_error("Could not read a search index", &error))?;
                chunks.append(&mut loaded);
            }
            Ok(chunks)
        })
        .await
        .map_err(|error| contextual_error("The search import stopped unexpectedly", &error))??;
    let hybrid = veda_search::HybridIndex::build(chunks)
        .map_err(|error| contextual_error("Could not build the search index", &error))?;
    let index = Arc::new(hybrid);
    *state.search_index.write() = Some(index.clone());
    Ok(index)
}

#[tauri::command]
pub async fn read_source(url: String, state: State<'_, AppState>) -> Result<ReaderSource, String> {
    if !url.starts_with("veda://docs/") {
        return Err("only local Veda documentation URLs may be opened".into());
    }
    let index = load_search_index(&state, &[]).await?;
    let chunk = index
        .find_by_url(&url)
        .ok_or_else(|| "the cited documentation chunk is no longer installed".to_string())?;
    Ok(ReaderSource {
        title: chunk.title.clone(),
        section: chunk.section.clone(),
        docset: format!("{} {}", chunk.docset.as_str(), chunk.version),
        text: chunk.text.clone(),
        url,
    })
}

#[tauri::command]
pub fn open_source(url: String) -> Result<(), String> {
    if !url.starts_with("veda://docs/") {
        return Err("only local Veda documentation URLs may be opened".into());
    }
    // The reader route consumes this URL in the UI; no external browser is used.
    Ok(())
}

#[tauri::command]
pub fn reveal_data_folder(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    app.opener()
        .open_path(state.data_dir.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|error| contextual_error("Could not open the data folder", &error))
}

fn docsets() -> Vec<Docset> {
    vec![
        Docset {
            id: "python".into(),
            name: "Python".into(),
            detail: "Language reference, standard library and tutorials from Python.org.".into(),
            version: "3.14.7".into(),
            compressed_bytes: 16_737_282,
            installed_bytes: 0,
            state: "available".into(),
            progress: 0.0,
            pages: None,
            accent: "#8fc7b0".into(),
            initials: "PY".into(),
        },
        Docset {
            id: "cpp".into(),
            name: "C++".into(),
            detail: "C and C++ language and standard library reference from cppreference.".into(),
            version: "cppreference 2025.02".into(),
            compressed_bytes: 55_740_889,
            installed_bytes: 0,
            state: "available".into(),
            progress: 0.0,
            pages: None,
            accent: "#81a7c8".into(),
            initials: "C++".into(),
        },
        Docset {
            id: "html".into(),
            name: "HTML".into(),
            detail: "Elements, attributes, forms, semantics and accessibility guides from MDN."
                .into(),
            version: "MDN 2026.08".into(),
            compressed_bytes: 73_684_713,
            installed_bytes: 0,
            state: "available".into(),
            progress: 0.0,
            pages: None,
            accent: "#dc9078".into(),
            initials: "<>".into(),
        },
        Docset {
            id: "css".into(),
            name: "CSS".into(),
            detail: "Properties, selectors, layout, animation and responsive design from MDN."
                .into(),
            version: "MDN 2026.08".into(),
            compressed_bytes: 73_684_713,
            installed_bytes: 0,
            state: "available".into(),
            progress: 0.0,
            pages: None,
            accent: "#889bd0".into(),
            initials: "#".into(),
        },
        Docset {
            id: "javascript".into(),
            name: "JavaScript".into(),
            detail: "JavaScript reference, operators, built-ins and language guides from MDN."
                .into(),
            version: "MDN 2026.08".into(),
            compressed_bytes: 73_684_713,
            installed_bytes: 0,
            state: "available".into(),
            progress: 0.0,
            pages: None,
            accent: "#d9c273".into(),
            initials: "JS".into(),
        },
    ]
}

#[allow(dead_code)]
fn is_safe_child(root: &Path, child: &Path) -> bool {
    child.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_files_put_the_requested_quant_first() {
        assert_eq!(
            model_files(Some(veda_core::ModelQuant::Q8))[0],
            "minicpm5-1b-Q8_0.gguf"
        );
        assert_eq!(
            model_files(Some(veda_core::ModelQuant::Q5))[0],
            "minicpm5-1b-Q5_K_M.gguf"
        );
        // Unknown/absent preference defaults to the Q5 file, the app default.
        assert_eq!(model_files(None)[0], "minicpm5-1b-Q5_K_M.gguf");
        // Both candidates are always present so a missing file falls back.
        assert_eq!(
            model_files(Some(veda_core::ModelQuant::Q5))[1],
            "minicpm5-1b-Q8_0.gguf"
        );
    }
}
