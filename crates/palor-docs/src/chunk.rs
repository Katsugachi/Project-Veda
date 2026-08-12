use crate::{DocChunk, DocPage};
use palor_core::DocsetId;

const TARGET_CHARS: usize = 3_200;
const OVERLAP_CHARS: usize = 240;

pub fn chunk_page(page: &DocPage, docset: DocsetId, version: &str) -> Vec<DocChunk> {
    let mut chunks = Vec::new();
    for section in &page.sections {
        let mut text = section.text.clone();
        for code in &section.code {
            text.push_str("\n\nCODE:\n");
            text.push_str(code);
        }
        let parts = split_bounded(&text);
        for (part_index, part) in parts.into_iter().enumerate() {
            chunks.push(DocChunk {
                id: format!(
                    "{}:{}:{}:{}",
                    docset.as_str(),
                    page.path,
                    section.anchor,
                    part_index
                ),
                docset,
                version: version.into(),
                page_path: page.path.clone(),
                title: page.title.clone(),
                section: section.heading.clone(),
                anchor: section.anchor.clone(),
                canonical_url: page.canonical_url.clone(),
                text: part,
                symbols: page.symbols.clone(),
            });
        }
    }
    chunks
}

fn split_bounded(text: &str) -> Vec<String> {
    let characters = text.chars().collect::<Vec<_>>();
    if characters.len() <= TARGET_CHARS {
        return if characters.is_empty() {
            Vec::new()
        } else {
            vec![text.into()]
        };
    }
    let mut parts = Vec::new();
    let mut start = 0;
    while start < characters.len() {
        let mut end = (start + TARGET_CHARS).min(characters.len());
        if end < characters.len() {
            let floor = start + TARGET_CHARS / 2;
            while end > floor && !matches!(characters[end - 1], '\n' | '.' | ';' | '}') {
                end -= 1;
            }
            if end == floor {
                end = (start + TARGET_CHARS).min(characters.len());
            }
        }
        parts.push(
            characters[start..end]
                .iter()
                .collect::<String>()
                .trim()
                .to_string(),
        );
        if end == characters.len() {
            break;
        }
        start = end.saturating_sub(OVERLAP_CHARS);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_sections_are_bounded_and_overlap() {
        let text = "sentence. ".repeat(800);
        let parts = split_bounded(&text);
        assert!(parts.len() > 1);
        assert!(parts
            .iter()
            .all(|part| part.chars().count() <= TARGET_CHARS + 10));
    }
}
