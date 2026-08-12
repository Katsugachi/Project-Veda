mod chunk;
mod format;
mod ingest;

pub use chunk::chunk_page;
pub use format::{
    DocChunk, DocPack, DocPackError, DocPackManifest, DocPage, DocSection, LicenseInfo,
};
pub use ingest::{ingest_directory, parse_html, parse_markdown, IngestError};
