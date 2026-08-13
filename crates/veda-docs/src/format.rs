use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
};
use veda_core::DocsetId;

pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
pub const MAX_PAGES_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseInfo {
    pub name: String,
    pub url: String,
    pub attribution: String,
    #[serde(default)]
    pub source_offer_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocPackManifest {
    pub schema_version: u32,
    pub id: DocsetId,
    pub name: String,
    pub version: String,
    pub source_url: String,
    pub source_revision: String,
    pub created_at: String,
    pub locale: String,
    pub page_count: usize,
    pub license: LicenseInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocSection {
    pub heading: String,
    pub anchor: String,
    pub text: String,
    #[serde(default)]
    pub code: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocPage {
    pub path: String,
    pub title: String,
    pub canonical_url: String,
    #[serde(default)]
    pub symbols: Vec<String>,
    pub sections: Vec<DocSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocChunk {
    pub id: String,
    pub docset: DocsetId,
    pub version: String,
    pub page_path: String,
    pub title: String,
    pub section: String,
    pub anchor: String,
    pub canonical_url: String,
    pub text: String,
    #[serde(default)]
    pub symbols: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DocPack {
    pub manifest: DocPackManifest,
    pub pages: Vec<DocPage>,
}

impl DocPack {
    pub fn write(&self, destination: &Path) -> Result<(), DocPackError> {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = destination.with_extension("vedadoc.part");
        let file = File::create(&temporary)?;
        let encoder = zstd::Encoder::new(file, 12)?;
        let mut archive = tar::Builder::new(encoder);
        let manifest = serde_json::to_vec_pretty(&self.manifest)?;
        append_bytes(&mut archive, "manifest.json", &manifest)?;
        let mut pages = Vec::new();
        for page in &self.pages {
            serde_json::to_writer(&mut pages, page)?;
            pages.push(b'\n');
        }
        append_bytes(&mut archive, "pages.jsonl", &pages)?;
        let encoder = archive.into_inner()?;
        encoder.finish()?;
        if destination.exists() {
            std::fs::remove_file(destination)?;
        }
        std::fs::rename(temporary, destination)?;
        Ok(())
    }

    pub fn read(source: &Path) -> Result<Self, DocPackError> {
        let file = File::open(source)?;
        let decoder = zstd::Decoder::new(file)?;
        let mut archive = tar::Archive::new(decoder);
        let mut manifest = None;
        let mut pages = Vec::new();
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.into_owned();
            match path.to_str() {
                Some("manifest.json") => {
                    if entry.size() > MAX_MANIFEST_BYTES {
                        return Err(DocPackError::EntryTooLarge("manifest.json".into()));
                    }
                    let mut content = Vec::new();
                    entry.read_to_end(&mut content)?;
                    manifest = Some(serde_json::from_slice(&content)?);
                }
                Some("pages.jsonl") => {
                    if entry.size() > MAX_PAGES_BYTES {
                        return Err(DocPackError::EntryTooLarge("pages.jsonl".into()));
                    }
                    let reader = BufReader::new(entry);
                    for line in reader.lines() {
                        let line = line?;
                        if !line.trim().is_empty() {
                            pages.push(serde_json::from_str(&line)?);
                        }
                    }
                }
                Some(other) => return Err(DocPackError::UnexpectedEntry(other.into())),
                None => return Err(DocPackError::UnexpectedEntry("non-UTF-8 path".into())),
            }
        }
        let manifest: DocPackManifest = manifest.ok_or(DocPackError::MissingManifest)?;
        if manifest.schema_version != 1 {
            return Err(DocPackError::UnsupportedSchema(manifest.schema_version));
        }
        if manifest.page_count != pages.len() {
            return Err(DocPackError::PageCount {
                expected: manifest.page_count,
                actual: pages.len(),
            });
        }
        Ok(Self { manifest, pages })
    }
}

fn append_bytes<W: Write>(
    archive: &mut tar::Builder<W>,
    path: &str,
    bytes: &[u8],
) -> Result<(), std::io::Error> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_cksum();
    archive.append_data(&mut header, path, bytes)
}

#[derive(Debug, thiserror::Error)]
pub enum DocPackError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("documentation pack is missing manifest.json")]
    MissingManifest,
    #[error("unsupported documentation pack schema {0}")]
    UnsupportedSchema(u32),
    #[error("unexpected archive entry: {0}")]
    UnexpectedEntry(String),
    #[error("archive entry is too large: {0}")]
    EntryTooLarge(String),
    #[error("manifest declares {expected} pages but archive contains {actual}")]
    PageCount { expected: usize, actual: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("python.vedadoc");
        let manifest = DocPackManifest {
            schema_version: 1,
            id: DocsetId::Python,
            name: "Python".into(),
            version: "3.14".into(),
            source_url: "https://docs.python.org".into(),
            source_revision: "3.14.7".into(),
            created_at: "2026-08-12".into(),
            locale: "en".into(),
            page_count: 1,
            license: LicenseInfo {
                name: "PSF-2.0".into(),
                url: "https://docs.python.org/3/license.html".into(),
                attribution: "Python Software Foundation".into(),
                source_offer_url: None,
            },
        };
        let page = DocPage {
            path: "library/asyncio-task.html".into(),
            title: "Coroutines and Tasks".into(),
            canonical_url: "https://docs.python.org/3/library/asyncio-task.html".into(),
            symbols: vec!["asyncio.TaskGroup".into()],
            sections: vec![DocSection {
                heading: "Task Groups".into(),
                anchor: "task-groups".into(),
                text: "A task group waits for all tasks.".into(),
                code: vec![],
            }],
        };
        DocPack {
            manifest,
            pages: vec![page],
        }
        .write(&destination)
        .unwrap();
        let loaded = DocPack::read(&destination).unwrap();
        assert_eq!(loaded.manifest.page_count, 1);
        assert_eq!(loaded.pages[0].symbols[0], "asyncio.TaskGroup");
    }
}
