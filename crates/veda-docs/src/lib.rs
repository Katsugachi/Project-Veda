mod chunk;
mod format;
mod ingest;

pub use chunk::chunk_page;
pub use format::{
    DocChunk, DocPack, DocPackError, DocPackManifest, DocPage, DocSection, LicenseInfo,
};
pub use ingest::{
    ingest_directory, ingest_memory, parse_html, parse_markdown, parse_plaintext, IngestError,
};
