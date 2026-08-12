use crate::{commands::DownloadItem, state::AppState};
use flate2::read::GzDecoder;
use palor_core::{default_catalog, Asset, AssetKind, ModelQuant};
use palor_downloads::{DownloadProgress, DownloadSpec, Downloader};
use palor_runtime::{LlamaSidecar, SidecarConfig};
use serde::Serialize;
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;

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

    let preferred = preferred_backend().await;
    let runtime = select_runtime(&catalog.assets, preferred)?.clone();
    match install_and_probe_runtime(&app, state, &catalog.assets, &runtime, &chat_path).await {
        Ok(active) => write_active_runtime(state, active).await,
        Err(accelerated_error) if preferred != "cpu" => {
            tracing::warn!(%accelerated_error, backend = preferred, "accelerated runtime failed health probe; falling back to CPU");
            let cpu = select_runtime(&catalog.assets, "cpu")?.clone();
            let active = install_and_probe_runtime(&app, state, &catalog.assets, &cpu, &chat_path)
                .await
                .map_err(|cpu_error| {
                    format!(
                        "{preferred} failed: {accelerated_error}; CPU fallback failed: {cpu_error}"
                    )
                })?;
            write_active_runtime(state, active).await
        }
        Err(error) => Err(error),
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
        .map_err(|error| error.to_string())?;
    for archive in archives {
        let _ = tokio::fs::remove_file(archive).await;
    }
    let executable_name = if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    };
    let executable = find_file(&runtime_dir, executable_name)
        .map_err(|error| error.to_string())?
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
    .map_err(|error| error.to_string())?;
    sidecar.stop().await.map_err(|error| error.to_string())?;
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
    let content = serde_json::to_vec_pretty(&runtime).map_err(|error| error.to_string())?;
    let destination = state.data_dir.join("runtime").join("active.json");
    let temporary = destination.with_extension("json.part");
    tokio::fs::write(&temporary, content)
        .await
        .map_err(|error| error.to_string())?;
    tokio::fs::rename(temporary, destination)
        .await
        .map_err(|error| error.to_string())
}

pub async fn install_docset(
    app: AppHandle,
    state: &AppState,
    id: palor_core::DocsetId,
) -> Result<(), String> {
    let source = default_catalog()
        .assets
        .into_iter()
        .find(|asset| asset.kind == AssetKind::Docset && asset.docset == Some(id))
        .ok_or_else(|| format!("no source is configured for {}", id.as_str()))?;
    let archive = download_asset(&app, state, &source, state.data_dir.join("downloads")).await?;
    let source_root = state
        .data_dir
        .join("docsets")
        .join(format!(".{}-source", id.as_str()));
    extract_runtime(&[archive], &source_root)
        .await
        .map_err(|error| error.to_string())?;
    let input = source_root.join(
        source
            .source_subdir
            .as_deref()
            .ok_or("docset source has no input directory")?,
    );
    let (version, canonical_base, license_url, attribution) = docset_metadata(id);
    let input_for_worker = input.clone();
    let canonical_for_worker = canonical_base.to_string();
    let pages = tokio::task::spawn_blocking(move || {
        palor_docs::ingest_directory(&input_for_worker, &canonical_for_worker)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;
    let manifest = palor_docs::DocPackManifest {
        schema_version: 1,
        id,
        name: docset_name(id).into(),
        version: version.into(),
        source_url: source.url.clone(),
        source_revision: source.id.clone(),
        created_at: "2026-08-12T00:00:00Z".into(),
        locale: "en".into(),
        page_count: pages.len(),
        license: palor_docs::LicenseInfo {
            name: source.license.clone(),
            url: license_url.into(),
            attribution: attribution.into(),
            source_offer_url: Some(source.url.clone()),
        },
    };
    let install_dir = state.data_dir.join("docsets").join(id.as_str());
    tokio::fs::create_dir_all(&install_dir)
        .await
        .map_err(|error| error.to_string())?;
    let pack_path = install_dir.join("content.palordoc");
    let pack = palor_docs::DocPack {
        manifest: manifest.clone(),
        pages,
    };
    let pack_for_worker = pack.clone();
    let path_for_worker = pack_path.clone();
    tokio::task::spawn_blocking(move || pack_for_worker.write(&path_for_worker))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    index_docset(&app, state, &pack).await?;
    tokio::fs::write(
        install_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?,
    )
    .await
    .map_err(|error| error.to_string())?;
    *state.search_index.write() = None;
    let _ = tokio::fs::remove_dir_all(source_root).await;
    Ok(())
}

async fn index_docset(
    app: &AppHandle,
    state: &AppState,
    pack: &palor_docs::DocPack,
) -> Result<(), String> {
    let active: ActiveRuntime = serde_json::from_slice(
        &tokio::fs::read(state.data_dir.join("runtime").join("active.json"))
            .await
            .map_err(|_| {
                "MiniCPM resources must be prepared before documentation indexing".to_string()
            })?,
    )
    .map_err(|error| error.to_string())?;
    let embedding_model = state
        .data_dir
        .join("models")
        .join("bge-small-en-v1.5-q8_0.gguf");
    let threads = sysinfo::System::new_all().cpus().len().clamp(1, 16);
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
    .map_err(|error| error.to_string())?;
    let client = palor_runtime::EmbeddingClient::new(&sidecar.base_url, &sidecar.api_key)
        .map_err(|error| error.to_string())?;
    let chunks = pack
        .pages
        .iter()
        .flat_map(|page| palor_docs::chunk_page(page, pack.manifest.id, &pack.manifest.version))
        .collect::<Vec<_>>();
    let id = pack.manifest.id.as_str().to_string();
    let item_name = format!("{} hybrid index", pack.manifest.name);
    let total = chunks.len().max(1);
    let mut indexed = Vec::with_capacity(chunks.len());
    for (batch_index, batch) in chunks.chunks(8).enumerate() {
        let inputs = batch
            .iter()
            .map(|chunk| {
                format!(
                    "{}\n{}\n{}",
                    chunk.title,
                    chunk.section,
                    chunk.text.chars().take(1_800).collect::<String>()
                )
            })
            .collect::<Vec<_>>();
        let embeddings = client
            .embed_passages(&inputs)
            .await
            .map_err(|error| error.to_string())?;
        if embeddings.len() != batch.len() {
            let _ = sidecar.stop().await;
            return Err("embedding server returned the wrong number of vectors".into());
        }
        for (chunk, embedding) in batch.iter().zip(embeddings) {
            indexed.push(palor_search::SearchChunk {
                id: chunk.id.clone(),
                docset: chunk.docset,
                version: chunk.version.clone(),
                title: chunk.title.clone(),
                section: chunk.section.clone(),
                url: format!(
                    "palor://docs/{}/{}#{}",
                    chunk.docset.as_str(),
                    chunk.page_path,
                    chunk.anchor
                ),
                text: chunk.text.clone(),
                symbols: chunk.symbols.clone(),
                embedding,
            });
        }
        let completed = ((batch_index + 1) * 8).min(total);
        let item = DownloadItem {
            id: format!("{id}-index"),
            name: item_name.clone(),
            detail: format!("Embedding {completed} of {total} chunks"),
            state: "indexing".into(),
            progress: completed as f32 / total as f32 * 100.0,
            downloaded_bytes: completed as u64,
            total_bytes: total as u64,
            speed_bytes: None,
        };
        upsert_download(state, item.clone());
        let _ = app.emit("download-progress", item);
    }
    sidecar.stop().await.map_err(|error| error.to_string())?;
    let index_dir = state.data_dir.join("indexes");
    tokio::fs::create_dir_all(&index_dir)
        .await
        .map_err(|error| error.to_string())?;
    let destination = index_dir.join(format!("{id}.json.zst"));
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let file = File::create(&destination).map_err(|error| error.to_string())?;
        let encoder = zstd::Encoder::new(file, 8).map_err(|error| error.to_string())?;
        serde_json::to_writer(encoder.auto_finish(), &indexed).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())??;
    let installed = DownloadItem {
        id: format!("{id}-index"),
        name: item_name,
        detail: format!("{} chunks indexed", total),
        state: "installed".into(),
        progress: 100.0,
        downloaded_bytes: total as u64,
        total_bytes: total as u64,
        speed_bytes: None,
    };
    upsert_download(state, installed.clone());
    let _ = app.emit("download-progress", installed);
    Ok(())
}

fn docset_name(id: palor_core::DocsetId) -> &'static str {
    match id {
        palor_core::DocsetId::Python => "Python",
        palor_core::DocsetId::Cpp => "C++",
        palor_core::DocsetId::Html => "HTML",
        palor_core::DocsetId::Css => "CSS",
        palor_core::DocsetId::Javascript => "JavaScript",
    }
}
fn docset_metadata(
    id: palor_core::DocsetId,
) -> (&'static str, &'static str, &'static str, &'static str) {
    match id {
        palor_core::DocsetId::Python => (
            "3.14.7",
            "https://docs.python.org/3",
            "https://docs.python.org/3/license.html",
            "© Python Software Foundation",
        ),
        palor_core::DocsetId::Cpp => (
            "2025-02-09",
            "https://en.cppreference.com/w",
            "https://en.cppreference.com/w/Cppreference:Copyright/CC-BY-SA",
            "cppreference.com contributors",
        ),
        palor_core::DocsetId::Html => (
            "2026-08-12",
            "https://developer.mozilla.org/en-US/docs",
            "https://creativecommons.org/licenses/by-sa/2.5/",
            "MDN content by Mozilla Contributors",
        ),
        palor_core::DocsetId::Css => (
            "2026-08-12",
            "https://developer.mozilla.org/en-US/docs",
            "https://creativecommons.org/licenses/by-sa/2.5/",
            "MDN content by Mozilla Contributors",
        ),
        palor_core::DocsetId::Javascript => (
            "2026-08-12",
            "https://developer.mozilla.org/en-US/docs",
            "https://creativecommons.org/licenses/by-sa/2.5/",
            "MDN content by Mozilla Contributors",
        ),
    }
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
    if free < palor_core::preflight::MINIMUM_FREE_DISK_BYTES {
        return Err("Palor requires at least 10 GiB free on its data drive before setup.".into());
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

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
async fn preferred_backend() -> &'static str {
    "cpu"
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
async fn preferred_backend() -> &'static str {
    let nvidia = tokio::process::Command::new("nvidia-smi.exe")
        .arg("--query-gpu=name")
        .arg("--format=csv,noheader")
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
        .map_err(|error| error.to_string())?;
    let filename = asset
        .url
        .rsplit('/')
        .next()
        .ok_or("asset URL has no filename")?;
    let destination = directory.join(filename);
    if destination.exists()
        && palor_downloads::verify_file(&destination, asset.bytes, &asset.sha256)
            .await
            .is_ok()
    {
        return Ok(destination);
    }
    let item = DownloadItem {
        id: asset.id.clone(),
        name: asset.name.clone(),
        detail: "Downloading and verifying".into(),
        state: "downloading".into(),
        progress: 0.0,
        downloaded_bytes: 0,
        total_bytes: asset.bytes,
        speed_bytes: None,
    };
    upsert_download(state, item.clone());
    let _ = app.emit("download-progress", &item);
    let downloader = Downloader::new().map_err(|error| error.to_string())?;
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
    let result = downloader
        .download(&spec, sender)
        .await
        .map_err(|error| error.to_string());
    progress_task.abort();
    result?;
    let installed = DownloadItem {
        id: asset.id.clone(),
        name: asset.name.clone(),
        detail: "Verified and ready".into(),
        state: "installed".into(),
        progress: 100.0,
        downloaded_bytes: asset.bytes,
        total_bytes: asset.bytes,
        speed_bytes: None,
    };
    upsert_download(state, installed.clone());
    let _ = app.emit("download-progress", installed);
    Ok(destination)
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
            let Some(relative) = entry.enclosed_name().map(Path::to_owned) else {
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
