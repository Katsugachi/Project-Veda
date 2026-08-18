use crate::{commands::DownloadItem, state::AppState};
use flate2::read::GzDecoder;
use serde::Serialize;
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use veda_core::{contextual_error, default_catalog, Asset, AssetKind, ModelQuant};
use veda_downloads::{DownloadError, DownloadProgress, DownloadSpec, Downloader};
use veda_runtime::{LlamaSidecar, SidecarConfig};

pub async fn prepare(app: AppHandle, state: &AppState, quant: ModelQuant) -> Result<(), String> {
    enforce_resources(quant, &state.data_dir)?;
    let catalog = default_catalog();
    let chat = catalog
        .assets
        .iter()
        .find(|asset| asset.kind == AssetKind::ChatModel && asset.quant == Some(quant))
        .cloned()
        .ok_or("model asset missing from catalog")?;
    let embedding = catalog
        .assets
        .iter()
        .find(|asset| asset.kind == AssetKind::EmbeddingModel)
        .cloned()
        .ok_or("embedding asset missing from catalog")?;
    let chat_path = download_asset(&app, state, &chat, state.data_dir.join("models")).await?;
    download_asset(&app, state, &embedding, state.data_dir.join("models")).await?;

    // Pick the best backend for this device and fall back through the chain
    // only when a real model-loading probe fails. Every failed runtime is
    // retired immediately, so exactly one backend is ever left installed —
    // the device never accumulates both CPU and GPU builds.
    let mut backend = preferred_backend().await;
    loop {
        let runtime = select_runtime(&catalog.assets, backend)?.clone();
        match install_and_probe_runtime(&app, state, &catalog.assets, &runtime, &chat_path).await {
            Ok(active) => {
                write_active_runtime(state, active).await?;
                return Ok(());
            }
            Err(error) => match next_backend(backend) {
                Some(next) => {
                    tracing::warn!(%error, backend, next, "runtime failed health probe; trying the next backend");
                    retire_runtime(state, &catalog.assets, &runtime).await;
                    backend = next;
                }
                None => return Err(error),
            },
        }
    }
}

/// The fallback chain after a runtime fails its health probe. `preferred_backend`
/// already chose the best fit for the platform, so this only steps down from a
/// failed accelerator to a more compatible one, ending at CPU.
fn next_backend(current: &str) -> Option<&'static str> {
    match current {
        "cuda" => Some("vulkan"),
        "vulkan" | "opencl-adreno" => Some("cpu"),
        _ => None,
    }
}

/// Removes a runtime that failed its health probe — plus any platform/backend
/// library dependency it pulled in (e.g. CUDA's `cudart`) — and their download
/// records, so exactly one backend remains installed after a fallback and the
/// Downloads tab never shows a phantom library row.
async fn retire_runtime(state: &AppState, assets: &[Asset], runtime: &Asset) {
    let mut ids = vec![runtime.id.clone()];
    ids.extend(
        assets
            .iter()
            .filter(|asset| {
                asset.kind == AssetKind::RuntimeDependency
                    && asset.platform == runtime.platform
                    && asset.backend == runtime.backend
            })
            .map(|asset| asset.id.clone()),
    );
    for id in ids {
        let dir = state.data_dir.join("runtime").join(&id);
        let _ = tokio::fs::remove_dir_all(&dir).await;
        state.downloads.write().retain(|item| item.id != id);
    }
}

async fn install_and_probe_runtime(
    app: &AppHandle,
    state: &AppState,
    assets: &[Asset],
    runtime: &Asset,
    model: &Path,
) -> Result<ActiveRuntime, String> {
    let mut archives = Vec::new();
    archives.push(download_asset(app, state, runtime, state.data_dir.join("downloads")).await?);
    for dependency in assets.iter().filter(|asset| {
        asset.kind == AssetKind::RuntimeDependency
            && asset.platform == runtime.platform
            && asset.backend == runtime.backend
    }) {
        archives
            .push(download_asset(app, state, dependency, state.data_dir.join("downloads")).await?);
    }
    let runtime_dir = state.data_dir.join("runtime").join(&runtime.id);
    extract_runtime(&archives, &runtime_dir)
        .await
        .map_err(|error| contextual_error("Could not unpack the downloaded runtime", &error))?;
    for archive in archives {
        let _ = tokio::fs::remove_file(archive).await;
    }
    let executable_name = if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    };
    let executable = find_file(&runtime_dir, executable_name)
        .map_err(|error| contextual_error("Could not inspect the downloaded runtime", &error))?
        .ok_or_else(|| format!("{executable_name} was not found in the downloaded runtime"))?;
    let accelerated = runtime.backend.as_deref() != Some("cpu");
    let threads = sysinfo::System::new_all().cpus().len().clamp(1, 16);
    let mut sidecar = LlamaSidecar::spawn(SidecarConfig {
        executable: executable.clone(),
        model: model.to_owned(),
        context_tokens: 4_096,
        gpu_layers: if accelerated { 99 } else { 0 },
        embedding: false,
        pooling: None,
        threads,
    })
    .await
    .map_err(|error| contextual_error("Could not start the llama.cpp runtime", &error))?;
    sidecar
        .stop()
        .await
        .map_err(|error| contextual_error("Could not stop the llama.cpp runtime", &error))?;
    Ok(ActiveRuntime {
        asset_id: runtime.id.clone(),
        backend: runtime.backend.clone().unwrap_or_else(|| "cpu".into()),
        executable,
    })
}

#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActiveRuntime {
    asset_id: String,
    backend: String,
    executable: PathBuf,
}

async fn write_active_runtime(state: &AppState, runtime: ActiveRuntime) -> Result<(), String> {
    let content = serde_json::to_vec_pretty(&runtime)
        .map_err(|error| contextual_error("Could not record the active runtime", &error))?;
    let destination = state.data_dir.join("runtime").join("active.json");
    let temporary = destination.with_extension("json.part");
    tokio::fs::write(&temporary, content)
        .await
        .map_err(|error| contextual_error("Could not record the active runtime", &error))?;
    tokio::fs::rename(temporary, destination)
        .await
        .map_err(|error| contextual_error("Could not record the active runtime", &error))
}

pub async fn install_docset(
    app: AppHandle,
    state: &AppState,
    id: veda_core::DocsetId,
) -> Result<(), String> {
    let source = default_catalog()
        .assets
        .into_iter()
        .find(|asset| asset.kind == AssetKind::Docset && asset.docset == Some(id))
        .ok_or_else(|| format!("no source is configured for {}", id.as_str()))?;
    let install_dir = state.data_dir.join("docsets").join(id.as_str());
    let index_path = state
        .data_dir
        .join("indexes")
        .join(format!("{}.json.zst", id.as_str()));
    let (version, canonical_base, license_url, attribution) = docset_metadata(id);
    let manifest_path = install_dir.join("manifest.json");
    if manifest_path.exists() && index_path.exists() {
        let installed_version = tokio::fs::read(&manifest_path)
            .await
            .ok()
            .and_then(|bytes| serde_json::from_slice::<veda_docs::DocPackManifest>(&bytes).ok())
            .map(|manifest| manifest.version);
        if installed_version.as_deref() == Some(version) {
            return Ok(());
        }
    }
    let archive = download_asset(&app, state, &source, state.data_dir.join("downloads")).await?;
    let source_root = state
        .data_dir
        .join("docsets")
        .join(format!(".{}-source", id.as_str()));
    extract_runtime(&[archive], &source_root)
        .await
        .map_err(|error| contextual_error("Could not unpack the documentation source", &error))?;
    let input = source_root.join(
        source
            .source_subdir
            .as_deref()
            .ok_or("docset source has no input directory")?,
    );
    let input_for_worker = input.clone();
    let canonical_for_worker = canonical_base.to_string();
    let pages = tokio::task::spawn_blocking(move || {
        veda_docs::ingest_directory(&input_for_worker, &canonical_for_worker)
    })
    .await
    .map_err(|error| contextual_error("The import stopped unexpectedly", &error))?
    .map_err(|error| contextual_error("Could not read the documentation source", &error))?;
    let manifest = veda_docs::DocPackManifest {
        schema_version: 1,
        id,
        name: docset_name(id).into(),
        version: version.into(),
        source_url: source.url.clone(),
        source_revision: source.id.clone(),
        created_at: "2026-08-12T00:00:00Z".into(),
        locale: "en".into(),
        page_count: pages.len(),
        license: veda_docs::LicenseInfo {
            name: source.license.clone(),
            url: license_url.into(),
            attribution: attribution.into(),
            source_offer_url: Some(source.url.clone()),
        },
    };
    tokio::fs::create_dir_all(&install_dir)
        .await
        .map_err(|error| contextual_error("Could not prepare the documentation folder", &error))?;
    let pack_path = install_dir.join("content.vedadoc");
    let pack = veda_docs::DocPack {
        manifest: manifest.clone(),
        pages,
    };
    let pack_for_worker = pack.clone();
    let path_for_worker = pack_path.clone();
    tokio::task::spawn_blocking(move || pack_for_worker.write(&path_for_worker))
        .await
        .map_err(|error| contextual_error("The documentation import stopped unexpectedly", &error))?
        .map_err(|error| contextual_error("Could not write the documentation pack", &error))?;
    index_docset(&app, state, &pack).await?;
    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| contextual_error("Could not save the documentation manifest", &error))?;
    tokio::fs::write(install_dir.join("manifest.json"), manifest_json)
        .await
        .map_err(|error| contextual_error("Could not save the documentation manifest", &error))?;
    *state.search_index.write() = None;
    let _ = tokio::fs::remove_dir_all(source_root).await;
    Ok(())
}

pub async fn remove_docset(state: &AppState, id: veda_core::DocsetId) -> Result<(), String> {
    let install_dir = state.data_dir.join("docsets").join(id.as_str());
    let source_dir = state
        .data_dir
        .join("docsets")
        .join(format!(".{}-source", id.as_str()));
    let index_path = state
        .data_dir
        .join("indexes")
        .join(format!("{}.json.zst", id.as_str()));

    if let Err(error) = tokio::fs::remove_dir_all(&install_dir).await {
        if error.kind() != io::ErrorKind::NotFound {
            return Err(contextual_error(
                "Could not remove the documentation pack",
                &error,
            ));
        }
    }
    if let Err(error) = tokio::fs::remove_dir_all(source_dir).await {
        if error.kind() != io::ErrorKind::NotFound {
            return Err(contextual_error(
                "Could not remove the documentation pack",
                &error,
            ));
        }
    }
    if let Err(error) = tokio::fs::remove_file(index_path).await {
        if error.kind() != io::ErrorKind::NotFound {
            return Err(contextual_error(
                "Could not remove the search index",
                &error,
            ));
        }
    }

    if let Some(source) = default_catalog()
        .assets
        .into_iter()
        .find(|asset| asset.kind == AssetKind::Docset && asset.docset == Some(id))
    {
        if let Some(filename) = source.url.rsplit('/').next() {
            let _ = tokio::fs::remove_file(state.data_dir.join("downloads").join(filename)).await;
        }
        let source_id = source.id;
        let index_id = format!("{}-index", id.as_str());
        state
            .downloads
            .write()
            .retain(|item| item.id != source_id && item.id != index_id);
    }
    *state.search_index.write() = None;
    Ok(())
}

async fn index_docset(
    app: &AppHandle,
    state: &AppState,
    pack: &veda_docs::DocPack,
) -> Result<(), String> {
    let chunks = pack
        .pages
        .iter()
        .flat_map(|page| veda_docs::chunk_page(page, pack.manifest.id, &pack.manifest.version))
        .collect::<Vec<_>>();
    // Semantic embeddings are optional. A missing model must never block
    // adding documentation — BM25 still works with empty vectors, and that
    // is exactly how a user "adds more docs" before first-run setup.
    let embedding_model = state
        .data_dir
        .join("models")
        .join("bge-small-en-v1.5-q8_0.gguf");
    let active = tokio::fs::read(state.data_dir.join("runtime").join("active.json"))
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ActiveRuntime>(&bytes).ok());
    let can_embed = embedding_model.exists() && active.is_some();
    if !can_embed {
        return write_lexical_index(app, state, pack, &chunks).await;
    }
    let active = active.expect("checked above");
    let logical_cpus = sysinfo::System::new_all().cpus().len();
    let threads = logical_cpus.div_ceil(2).clamp(1, 8);
    let mut sidecar = LlamaSidecar::spawn(SidecarConfig {
        executable: active.executable.clone(),
        model: embedding_model,
        context_tokens: 512,
        gpu_layers: if active.backend == "cpu" { 0 } else { 99 },
        embedding: true,
        pooling: Some("cls".into()),
        threads,
    })
    .await
    .map_err(|error| contextual_error("Could not start search preparation", &error))?;
    let client = veda_runtime::EmbeddingClient::new(&sidecar.base_url, &sidecar.api_key)
        .map_err(|error| contextual_error("Could not start search preparation", &error))?;
    let id = index_stem(&pack.manifest);
    let item_name = pack.manifest.name.clone();
    let total = chunks.len().max(1);
    let mut indexed = Vec::with_capacity(chunks.len());
    let mut last_progress = Instant::now() - Duration::from_secs(1);
    for (batch_index, batch) in chunks.chunks(4).enumerate() {
        let inputs = batch
            .iter()
            .map(|chunk| {
                let complete = format!("{}\n{}\n{}", chunk.title, chunk.section, chunk.text);
                veda_runtime::bounded_embedding_input(
                    &complete,
                    veda_runtime::MAX_EMBEDDING_INPUT_BYTES,
                )
            })
            .collect::<Vec<_>>();
        let embeddings = match client.embed_passages(&inputs).await {
            Ok(embeddings) => embeddings,
            Err(batch_error) => {
                tracing::warn!(%batch_error, "embedding batch failed; retrying one section at a time");
                let mut recovered = Vec::with_capacity(inputs.len());
                for input in &inputs {
                    recovered.push(embed_one_with_backoff(&client, input).await?);
                }
                recovered
            }
        };
        if embeddings.len() != batch.len() {
            let _ = sidecar.stop().await;
            return Err("search preparation returned an unexpected result".into());
        }
        for (chunk, embedding) in batch.iter().zip(embeddings) {
            indexed.push(veda_search::SearchChunk {
                id: chunk.id.clone(),
                docset: chunk.docset,
                version: chunk.version.clone(),
                title: chunk.title.clone(),
                section: chunk.section.clone(),
                url: format!("veda://docs/{id}/{}#{}", chunk.page_path, chunk.anchor),
                text: chunk.text.clone(),
                symbols: chunk.symbols.clone(),
                embedding,
            });
        }
        let completed = ((batch_index + 1) * 4).min(total);
        if last_progress.elapsed() >= Duration::from_millis(250) || completed == total {
            let item = DownloadItem {
                id: format!("{id}-index"),
                name: item_name.clone(),
                detail: format!("Preparing search · {completed} of {total}"),
                state: "indexing".into(),
                progress: completed as f32 / total as f32 * 100.0,
                downloaded_bytes: completed as u64,
                total_bytes: total as u64,
                speed_bytes: None,
                docset: Some(id.clone()),
            };
            upsert_download(state, item.clone());
            let _ = app.emit("download-progress", item);
            last_progress = Instant::now();
        }
    }
    sidecar
        .stop()
        .await
        .map_err(|error| contextual_error("Could not stop search preparation", &error))?;
    let index_dir = state.data_dir.join("indexes");
    tokio::fs::create_dir_all(&index_dir)
        .await
        .map_err(|error| contextual_error("Could not prepare the search index folder", &error))?;
    let destination = index_dir.join(format!("{id}.json.zst"));
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let file = File::create(&destination)
            .map_err(|error| contextual_error("Could not write the search index", &error))?;
        let encoder = zstd::Encoder::new(file, 8)
            .map_err(|error| contextual_error("Could not write the search index", &error))?;
        serde_json::to_writer(encoder.auto_finish(), &indexed)
            .map_err(|error| contextual_error("Could not write the search index", &error))
    })
    .await
    .map_err(|error| contextual_error("The search import stopped unexpectedly", &error))??;
    let installed = DownloadItem {
        id: format!("{id}-index"),
        name: item_name,
        detail: "Ready".into(),
        state: "installed".into(),
        progress: 100.0,
        downloaded_bytes: total as u64,
        total_bytes: total as u64,
        speed_bytes: None,
        docset: Some(id.clone()),
    };
    upsert_download(state, installed.clone());
    let _ = app.emit("download-progress", installed);
    Ok(())
}

const MIN_EMBEDDING_RETRY_BYTES: usize = 128;

fn next_embedding_retry_limit(current: usize) -> usize {
    (current * 3 / 4).max(MIN_EMBEDDING_RETRY_BYTES)
}

async fn embed_one_with_backoff(
    client: &veda_runtime::EmbeddingClient,
    input: &str,
) -> Result<Vec<f32>, String> {
    let mut max_bytes = input.len();
    loop {
        let candidate = veda_runtime::bounded_embedding_input(input, max_bytes);
        match client.embed_passages(&[candidate]).await {
            Ok(mut embeddings) if embeddings.len() == 1 => return Ok(embeddings.remove(0)),
            Ok(_) => return Err("Search preparation returned an unexpected result.".into()),
            Err(error) if max_bytes > MIN_EMBEDDING_RETRY_BYTES => {
                let next_max_bytes = next_embedding_retry_limit(max_bytes);
                tracing::warn!(%error, max_bytes, next_max_bytes, "section was too large; retrying with a shorter input");
                max_bytes = next_max_bytes;
            }
            Err(error) => {
                return Err(format!(
                    "Could not prepare search for this documentation. {}",
                    contextual_error("a section could not be analyzed", &error)
                ));
            }
        }
    }
}

/// On-disk index filename stem. Official packs use the catalog id (`python`).
/// User libraries use the folder slug (`local-notes`), never the shared
/// `DocsetId::Local` string `"local"` — that collision is what made a second
/// local library overwrite the first, and made list_local_docsets miss both.
pub(crate) fn index_stem(manifest: &veda_docs::DocPackManifest) -> String {
    if manifest.version.starts_with("local-") {
        manifest.version.clone()
    } else {
        manifest.id.as_str().to_string()
    }
}

async fn write_lexical_index(
    app: &AppHandle,
    state: &AppState,
    pack: &veda_docs::DocPack,
    chunks: &[veda_docs::DocChunk],
) -> Result<(), String> {
    let item_id = index_stem(&pack.manifest);
    let total = chunks.len().max(1);
    let indexed = chunks
        .iter()
        .map(|chunk| veda_search::SearchChunk {
            id: chunk.id.clone(),
            docset: chunk.docset,
            version: chunk.version.clone(),
            title: chunk.title.clone(),
            section: chunk.section.clone(),
            url: format!("veda://docs/{item_id}/{}#{}", chunk.page_path, chunk.anchor),
            text: chunk.text.clone(),
            symbols: chunk.symbols.clone(),
            embedding: Vec::new(),
        })
        .collect::<Vec<_>>();
    persist_index(state, &item_id, indexed).await?;
    let installed = DownloadItem {
        id: format!("{item_id}-index"),
        name: pack.manifest.name.clone(),
        detail: format!("{total} pages ready (keyword search)"),
        state: "installed".into(),
        progress: 100.0,
        downloaded_bytes: total as u64,
        total_bytes: total as u64,
        speed_bytes: None,
        docset: Some(item_id),
    };
    upsert_download(state, installed.clone());
    let _ = app.emit("download-progress", installed);
    Ok(())
}

async fn persist_index(
    state: &AppState,
    id: &str,
    indexed: Vec<veda_search::SearchChunk>,
) -> Result<(), String> {
    let index_dir = state.data_dir.join("indexes");
    tokio::fs::create_dir_all(&index_dir)
        .await
        .map_err(|error| contextual_error("Could not prepare the search index folder", &error))?;
    let destination = index_dir.join(format!("{id}.json.zst"));
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let file = File::create(&destination)
            .map_err(|error| contextual_error("Could not write the search index", &error))?;
        let encoder = zstd::Encoder::new(file, 8)
            .map_err(|error| contextual_error("Could not write the search index", &error))?;
        serde_json::to_writer(encoder.auto_finish(), &indexed)
            .map_err(|error| contextual_error("Could not write the search index", &error))
    })
    .await
    .map_err(|error| contextual_error("The search import stopped unexpectedly", &error))??;
    Ok(())
}

fn docset_name(id: veda_core::DocsetId) -> &'static str {
    match id {
        veda_core::DocsetId::Python => "Python",
        veda_core::DocsetId::Cpp => "C++",
        veda_core::DocsetId::Html => "HTML",
        veda_core::DocsetId::Css => "CSS",
        veda_core::DocsetId::Javascript => "JavaScript",
        veda_core::DocsetId::Local => "Local docs",
    }
}
pub(crate) fn expected_docset_version(id: &str) -> Option<&'static str> {
    let id = match id {
        "python" => veda_core::DocsetId::Python,
        "cpp" => veda_core::DocsetId::Cpp,
        "html" => veda_core::DocsetId::Html,
        "css" => veda_core::DocsetId::Css,
        "javascript" => veda_core::DocsetId::Javascript,
        _ => return None,
    };
    Some(docset_metadata(id).0)
}

fn docset_metadata(
    id: veda_core::DocsetId,
) -> (&'static str, &'static str, &'static str, &'static str) {
    match id {
        veda_core::DocsetId::Python => (
            "3.14.7",
            "https://docs.python.org/3",
            "https://docs.python.org/3/license.html",
            "© Python Software Foundation",
        ),
        veda_core::DocsetId::Cpp => (
            "2025-02-09",
            "https://en.cppreference.com/w",
            "https://en.cppreference.com/w/Cppreference:Copyright/CC-BY-SA",
            "cppreference.com contributors",
        ),
        veda_core::DocsetId::Html => (
            "2026-08-12",
            "https://developer.mozilla.org/en-US/docs",
            "https://creativecommons.org/licenses/by-sa/2.5/",
            "MDN content by Mozilla Contributors",
        ),
        veda_core::DocsetId::Css => (
            "2026-08-12",
            "https://developer.mozilla.org/en-US/docs",
            "https://creativecommons.org/licenses/by-sa/2.5/",
            "MDN content by Mozilla Contributors",
        ),
        veda_core::DocsetId::Javascript => (
            "2026-08-12",
            "https://developer.mozilla.org/en-US/docs",
            "https://creativecommons.org/licenses/by-sa/2.5/",
            "MDN content by Mozilla Contributors",
        ),
        veda_core::DocsetId::Local => ("local", "veda://local", "", "Imported by the user"),
    }
}

pub fn slugify_library(name: &str) -> String {
    let slug = name
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "docs".into()
    } else {
        slug
    }
}

/// Imports a user-chosen folder or an in-memory file list as a new local
/// documentation library. Works without MiniCPM: the index is lexical-only
/// until the embedding model is installed.
pub async fn install_local_docs(
    app: AppHandle,
    state: &AppState,
    name: String,
    files: Option<Vec<(String, String)>>,
    folder: Option<PathBuf>,
) -> Result<crate::commands::Docset, String> {
    let slug = slugify_library(&name);
    let mut library_id = format!("local-{slug}");
    let mut suffix = 2_u32;
    while state
        .data_dir
        .join("docsets")
        .join(&library_id)
        .join("manifest.json")
        .exists()
    {
        library_id = format!("local-{slug}-{suffix}");
        suffix += 1;
    }
    let canonical = format!("veda://docs/{library_id}");
    let pages = if let Some(files) = files {
        veda_docs::ingest_memory(&files, &canonical)
            .map_err(|error| contextual_error("Could not read the documentation files", &error))?
    } else if let Some(folder) = folder {
        let folder_for_worker = folder.clone();
        let canonical_for_worker = canonical.clone();
        tokio::task::spawn_blocking(move || {
            veda_docs::ingest_directory(&folder_for_worker, &canonical_for_worker)
        })
        .await
        .map_err(|error| contextual_error("The import stopped unexpectedly", &error))?
        .map_err(|error| contextual_error("Could not read the documentation folder", &error))?
    } else {
        return Err("Choose a folder or some documentation files first.".into());
    };
    if pages.is_empty() {
        return Err("No Markdown, HTML or text files were found in that selection.".into());
    }
    let page_count = pages.len();
    let install_dir = state.data_dir.join("docsets").join(&library_id);
    tokio::fs::create_dir_all(&install_dir)
        .await
        .map_err(|error| contextual_error("Could not prepare the documentation folder", &error))?;
    let manifest = veda_docs::DocPackManifest {
        schema_version: 1,
        id: veda_core::DocsetId::Local,
        name: name.clone(),
        version: library_id.clone(),
        source_url: canonical.clone(),
        source_revision: library_id.clone(),
        created_at: "2026-08-18T00:00:00Z".into(),
        locale: "en".into(),
        page_count,
        license: veda_docs::LicenseInfo {
            name: "User imported".into(),
            url: String::new(),
            attribution: "Imported by the user".into(),
            source_offer_url: None,
        },
    };
    let pack = veda_docs::DocPack {
        manifest: manifest.clone(),
        pages,
    };
    let pack_path = install_dir.join("content.vedadoc");
    let pack_for_worker = pack.clone();
    tokio::task::spawn_blocking(move || pack_for_worker.write(&pack_path))
        .await
        .map_err(|error| contextual_error("The documentation import stopped unexpectedly", &error))?
        .map_err(|error| contextual_error("Could not write the documentation pack", &error))?;
    index_docset(&app, state, &pack).await?;
    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| contextual_error("Could not save the documentation manifest", &error))?;
    tokio::fs::write(install_dir.join("manifest.json"), manifest_json)
        .await
        .map_err(|error| contextual_error("Could not save the documentation manifest", &error))?;
    *state.search_index.write() = None;
    Ok(crate::commands::Docset {
        id: library_id,
        name,
        detail: "Your documentation, searchable offline.".into(),
        version: "local".into(),
        compressed_bytes: 0,
        installed_bytes: 0,
        state: "installed".into(),
        progress: 100.0,
        pages: Some(page_count),
        accent: "#c4a574".into(),
        initials: "YO".into(),
    })
}

pub async fn remove_local_docset(state: &AppState, id: &str) -> Result<(), String> {
    if !id.starts_with("local-") && id != "local" {
        return Err(format!("not a local documentation library: {id}"));
    }
    let install_dir = state.data_dir.join("docsets").join(id);
    let index_path = state
        .data_dir
        .join("indexes")
        .join(format!("{id}.json.zst"));
    if let Err(error) = tokio::fs::remove_dir_all(&install_dir).await {
        if error.kind() != io::ErrorKind::NotFound {
            return Err(contextual_error(
                "Could not remove the documentation pack",
                &error,
            ));
        }
    }
    if let Err(error) = tokio::fs::remove_file(index_path).await {
        if error.kind() != io::ErrorKind::NotFound {
            return Err(contextual_error(
                "Could not remove the search index",
                &error,
            ));
        }
    }
    let index_id = format!("{id}-index");
    state
        .downloads
        .write()
        .retain(|item| item.id != id && item.id != index_id);
    *state.search_index.write() = None;
    Ok(())
}

fn enforce_resources(quant: ModelQuant, data_dir: &Path) -> Result<(), String> {
    let mut system = sysinfo::System::new_all();
    system.refresh_memory();
    let required_memory = match quant {
        ModelQuant::Q5 => 6,
        ModelQuant::Q8 => 12,
    } * 1024_u64.pow(3);
    if system.total_memory() < required_memory {
        return Err(format!(
            "This model selection requires at least {} GiB of physical memory.",
            required_memory / 1024_u64.pow(3)
        ));
    }
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let free = disks
        .iter()
        .filter(|disk| data_dir.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .or_else(|| disks.iter().max_by_key(|disk| disk.available_space()))
        .map_or(0, |disk| disk.available_space());
    if free < veda_core::preflight::MINIMUM_FREE_DISK_BYTES {
        return Err("Veda requires at least 10 GiB free on its data drive before setup.".into());
    }
    Ok(())
}

fn platform_key() -> Result<&'static str, String> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("macos-arm64"),
        ("macos", "x86_64") => Ok("macos-x64"),
        ("windows", "aarch64") => Ok("windows-arm64"),
        ("windows", "x86_64") => Ok("windows-x64"),
        _ => Err(format!(
            "unsupported runtime platform: {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )),
    }
}

fn select_runtime<'a>(assets: &'a [Asset], backend: &str) -> Result<&'a Asset, String> {
    let platform = platform_key()?;
    assets
        .iter()
        .find(|asset| {
            asset.kind == AssetKind::LlamaRuntime
                && asset.platform.as_deref() == Some(platform)
                && asset.backend.as_deref() == Some(backend)
        })
        .ok_or_else(|| format!("no {backend} llama.cpp runtime for {platform}"))
}

#[cfg(target_os = "macos")]
async fn preferred_backend() -> &'static str {
    "metal"
}

// Windows on ARM64 (Snapdragon X and friends) runs the OpenCL Adreno build of
// llama.cpp so the engine is GPU-accelerated like every other supported
// platform. The build is self-contained (no separate runtime dependency), and
// the setup health probe rolls back to the catalogued CPU runtime if the
// device has no usable Adreno/OpenCL driver, so a non-Adreno ARM64 box still
// works.
#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
async fn preferred_backend() -> &'static str {
    "opencl-adreno"
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
async fn preferred_backend() -> &'static str {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = tokio::process::Command::new("nvidia-smi.exe");
    command.arg("--query-gpu=name").arg("--format=csv,noheader");
    command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
    let nvidia = command
        .output()
        .await
        .is_ok_and(|output| output.status.success() && !output.stdout.is_empty());
    if nvidia {
        "cuda"
    } else {
        "vulkan"
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
async fn preferred_backend() -> &'static str {
    "cpu"
}

async fn download_asset(
    app: &AppHandle,
    state: &AppState,
    asset: &Asset,
    directory: PathBuf,
) -> Result<PathBuf, String> {
    tokio::fs::create_dir_all(&directory)
        .await
        .map_err(|error| contextual_error("Could not prepare the download folder", &error))?;
    let filename = asset
        .url
        .rsplit('/')
        .next()
        .ok_or("asset URL has no filename")?;
    let destination = directory.join(filename);
    if destination.exists()
        && veda_downloads::verify_file(&destination, asset.bytes, &asset.sha256)
            .await
            .is_ok()
    {
        return Ok(destination);
    }
    let docset_id = asset.docset.map(|docset| docset.as_str().to_string());
    let item = DownloadItem {
        id: asset.id.clone(),
        name: asset.name.clone(),
        detail: "Downloading and verifying".into(),
        state: "downloading".into(),
        progress: 0.0,
        downloaded_bytes: 0,
        total_bytes: asset.bytes,
        speed_bytes: None,
        docset: docset_id.clone(),
    };
    upsert_download(state, item.clone());
    let _ = app.emit("download-progress", &item);
    let downloader = Downloader::new()
        .map_err(|error| contextual_error("Could not start the download client", &error))?;
    let (sender, mut receiver) = watch::channel(DownloadProgress {
        id: asset.id.clone(),
        downloaded_bytes: 0,
        total_bytes: asset.bytes,
        bytes_per_second: 0,
        verifying: false,
    });
    let progress_app = app.clone();
    let progress_state = state.downloads.clone();
    let name = asset.name.clone();
    let progress_docset = docset_id.clone();
    let progress_task = tokio::spawn(async move {
        while receiver.changed().await.is_ok() {
            let progress = receiver.borrow_and_update().clone();
            let item = DownloadItem {
                id: progress.id.clone(),
                name: name.clone(),
                detail: if progress.verifying {
                    "Verifying SHA-256".into()
                } else {
                    "Downloading".into()
                },
                state: if progress.verifying {
                    "verifying".into()
                } else {
                    "downloading".into()
                },
                progress: progress.downloaded_bytes as f32 / progress.total_bytes.max(1) as f32
                    * 100.0,
                downloaded_bytes: progress.downloaded_bytes,
                total_bytes: progress.total_bytes,
                speed_bytes: Some(progress.bytes_per_second),
                docset: progress_docset.clone(),
            };
            let mut items = progress_state.write();
            if let Some(existing) = items.iter_mut().find(|existing| existing.id == item.id) {
                *existing = item.clone();
            } else {
                items.push(item.clone());
            }
            drop(items);
            let _ = progress_app.emit("download-progress", item);
        }
    });
    let spec = DownloadSpec {
        id: asset.id.clone(),
        url: asset.url.clone(),
        destination: destination.clone(),
        expected_bytes: asset.bytes,
        sha256: asset.sha256.clone(),
    };
    let result = download_with_retry(&downloader, &spec, sender).await;
    progress_task.abort();
    if let Err(error) = result {
        // Reflect the failure in the downloads list so it never looks like a
        // download that is still running. Partial files stay on disk, so the
        // next attempt resumes instead of restarting.
        let part_path = std::path::PathBuf::from(format!("{}.part", destination.display()));
        let partial_bytes = tokio::fs::metadata(&part_path)
            .await
            .map_or(0, |meta| meta.len());
        let failed = DownloadItem {
            id: asset.id.clone(),
            name: asset.name.clone(),
            detail: "Stopped, will resume on retry".into(),
            state: "error".into(),
            progress: partial_bytes as f32 / asset.bytes.max(1) as f32 * 100.0,
            downloaded_bytes: partial_bytes,
            total_bytes: asset.bytes,
            speed_bytes: None,
            docset: docset_id.clone(),
        };
        upsert_download(state, failed);
        return Err(contextual_error(
            &format!("Could not download {}", asset.name),
            &error,
        ));
    }
    let installed = DownloadItem {
        id: asset.id.clone(),
        name: asset.name.clone(),
        detail: String::new(),
        state: "installed".into(),
        progress: 100.0,
        downloaded_bytes: asset.bytes,
        total_bytes: asset.bytes,
        speed_bytes: None,
        docset: docset_id,
    };
    upsert_download(state, installed.clone());
    let _ = app.emit("download-progress", installed);
    Ok(destination)
}

/// Downloads with a bounded number of attempts. Transient transport and
/// filesystem failures retry automatically and resume from the `.part` file;
/// permanent failures (bad checksums, HTTP errors, oversized responses)
/// surface immediately.
async fn download_with_retry(
    downloader: &Downloader,
    spec: &DownloadSpec,
    sender: tokio::sync::watch::Sender<DownloadProgress>,
) -> Result<(), DownloadError> {
    const MAX_ATTEMPTS: usize = 3;
    let mut attempt = 0;
    loop {
        attempt += 1;
        match downloader.download(spec, sender.clone()).await {
            Ok(()) => return Ok(()),
            Err(error) if attempt < MAX_ATTEMPTS && is_transient(&error) => {
                tracing::warn!(attempt, %error, asset = %spec.id, "retrying download");
                tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

fn is_transient(error: &DownloadError) -> bool {
    matches!(error, DownloadError::Request(_) | DownloadError::Io(_))
}

fn upsert_download(state: &AppState, item: DownloadItem) {
    let mut items = state.downloads.write();
    if let Some(existing) = items.iter_mut().find(|existing| existing.id == item.id) {
        *existing = item;
    } else {
        items.push(item);
    }
}

async fn extract_runtime(archives: &[PathBuf], destination: &Path) -> io::Result<()> {
    let archives = archives.to_vec();
    let destination = destination.to_owned();
    tokio::task::spawn_blocking(move || {
        let temporary = destination.with_extension("extracting");
        if temporary.exists() {
            std::fs::remove_dir_all(&temporary)?;
        }
        std::fs::create_dir_all(&temporary)?;
        for archive in archives {
            extract_archive(&archive, &temporary)?;
        }
        #[cfg(unix)]
        make_server_executable(&temporary)?;
        if destination.exists() {
            std::fs::remove_dir_all(&destination)?;
        }
        std::fs::rename(temporary, destination)
    })
    .await
    .map_err(io::Error::other)?
}

fn extract_archive(archive: &Path, destination: &Path) -> io::Result<()> {
    let filename = archive
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if filename.ends_with(".zip") {
        let file = File::open(archive)?;
        let mut zip = zip::ZipArchive::new(file).map_err(io::Error::other)?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(io::Error::other)?;
            let Some(relative) = entry.enclosed_name() else {
                return Err(io::Error::other("unsafe path in runtime ZIP"));
            };
            let output = destination.join(relative);
            if entry.is_dir() {
                std::fs::create_dir_all(&output)?;
                continue;
            }
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = File::create(output)?;
            io::copy(&mut entry, &mut file)?;
        }
    } else if filename.ends_with(".tar.gz") {
        let file = File::open(archive)?;
        let mut archive = tar::Archive::new(GzDecoder::new(file));
        archive.unpack(destination)?;
    } else {
        return Err(io::Error::other("unsupported runtime archive"));
    }
    Ok(())
}

fn find_file(root: &Path, filename: &str) -> io::Result<Option<PathBuf>> {
    Ok(walk_files(root)?
        .into_iter()
        .find(|path| path.file_name().and_then(|value| value.to_str()) == Some(filename)))
}

#[cfg(unix)]
fn make_server_executable(root: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    for entry in walk_files(root)? {
        if entry.file_name().and_then(|name| name.to_str()) == Some("llama-server") {
            let mut permissions = std::fs::metadata(&entry)?.permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(entry, permissions)?;
        }
    }
    Ok(())
}

fn walk_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else {
                files.push(entry.path());
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::{next_backend, next_embedding_retry_limit, MIN_EMBEDDING_RETRY_BYTES};

    #[test]
    fn backend_fallback_chain_steps_down_to_cpu() {
        assert_eq!(next_backend("cuda"), Some("vulkan"));
        assert_eq!(next_backend("vulkan"), Some("cpu"));
        assert_eq!(next_backend("opencl-adreno"), Some("cpu"));
        assert_eq!(next_backend("cpu"), None);
        // macOS has no CPU runtime in the catalog; Metal failing is a hard
        // error rather than a silent CPU downgrade.
        assert_eq!(next_backend("metal"), None);
    }

    #[test]
    fn library_slugs_are_stable_and_safe() {
        assert_eq!(super::slugify_library("My Notes!"), "my-notes");
        assert_eq!(super::slugify_library("///"), "docs");
        assert_eq!(super::slugify_library("C++ API"), "c-api");
    }

    #[test]
    fn local_index_stem_uses_the_folder_slug_not_the_shared_enum() {
        // list_local_docsets looks for `local-<slug>.json.zst`. Writing
        // `local.json.zst` (DocsetId::Local.as_str()) made the second library
        // overwrite the first and made both vanish from the Docs tab.
        let local = veda_docs::DocPackManifest {
            schema_version: 1,
            id: veda_core::DocsetId::Local,
            name: "Notes".into(),
            version: "local-notes".into(),
            source_url: "veda://docs/local-notes".into(),
            source_revision: "local-notes".into(),
            created_at: "2026-08-18T00:00:00Z".into(),
            locale: "en".into(),
            page_count: 1,
            license: veda_docs::LicenseInfo {
                name: "User imported".into(),
                url: String::new(),
                attribution: "Imported by the user".into(),
                source_offer_url: None,
            },
        };
        assert_eq!(super::index_stem(&local), "local-notes");
        assert_ne!(
            super::index_stem(&local),
            veda_core::DocsetId::Local.as_str()
        );

        let python = veda_docs::DocPackManifest {
            schema_version: 1,
            id: veda_core::DocsetId::Python,
            name: "Python".into(),
            version: "3.14.7".into(),
            source_url: "https://docs.python.org/3".into(),
            source_revision: "python-source-3.14.7".into(),
            created_at: "2026-08-12T00:00:00Z".into(),
            locale: "en".into(),
            page_count: 1,
            license: veda_docs::LicenseInfo {
                name: "PSF".into(),
                url: String::new(),
                attribution: String::new(),
                source_offer_url: None,
            },
        };
        assert_eq!(super::index_stem(&python), "python");
    }

    #[test]
    fn embedding_retry_limit_strictly_shrinks_and_terminates() {
        let mut limit = veda_runtime::MAX_EMBEDDING_INPUT_BYTES;
        let mut attempts = 0;
        while limit > MIN_EMBEDDING_RETRY_BYTES {
            let next = next_embedding_retry_limit(limit);
            assert!(next < limit, "retry must never repeat the same input limit");
            limit = next;
            attempts += 1;
            assert!(attempts < 10, "retry schedule must be finite");
        }
        assert_eq!(limit, MIN_EMBEDDING_RETRY_BYTES);
    }
}
