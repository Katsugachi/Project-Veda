use crate::{DocChunk, DocPage};
use veda_core::DocsetId;

const TARGET_CHARS: usize = 5_200;
const OVERLAP_CHARS: usize = 160;

/// Groups adjacent short sections before indexing. Documentation pages often
/// contain dozens of tiny headings; embedding each heading independently makes
/// first-run setup unnecessarily slow without improving retrieval.
pub fn chunk_page(page: &DocPage, docset: DocsetId, version: &str) -> Vec<DocChunk> {
    let mut chunks = Vec::new();
    let mut buffer = String::new();
    let mut headings = Vec::<String>::new();
    let mut anchor = String::from("top");

    for section in &page.sections {
        let mut payload = section.text.clone();
        for code in &section.code {
            payload.push_str("\n\nCODE:\n");
            payload.push_str(code);
        }
        for part in split_bounded(&payload) {
            let heading_prefix = format!("{}\n", section.heading);
            let added_chars = heading_prefix.chars().count() + part.chars().count() + 2;
            if !buffer.is_empty() && buffer.chars().count() + added_chars > TARGET_CHARS {
                push_chunk(
                    &mut chunks,
                    page,
                    docset,
                    version,
                    &headings,
                    &anchor,
                    std::mem::take(&mut buffer),
                );
                headings.clear();
            }
            if buffer.is_empty() {
                anchor = section.anchor.clone();
            }
            if headings.last() != Some(&section.heading) {
                headings.push(section.heading.clone());
                buffer.push_str(&heading_prefix);
            }
            buffer.push_str(&part);
            buffer.push_str("\n\n");
        }
    }

    if !buffer.trim().is_empty() {
        push_chunk(
            &mut chunks,
            page,
            docset,
            version,
            &headings,
            &anchor,
            buffer,
        );
    }
    chunks
}

fn push_chunk(
    chunks: &mut Vec<DocChunk>,
    page: &DocPage,
    docset: DocsetId,
    version: &str,
    headings: &[String],
    anchor: &str,
    text: String,
) {
    let index = chunks.len();
    let section = if headings.is_empty() {
        page.title.clone()
    } else {
        headings
            .iter()
            .take(6)
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ")
    };
    chunks.push(DocChunk {
        id: format!("{}:{}:{}", docset.as_str(), page.path, index),
        docset,
        version: version.into(),
        page_path: page.path.clone(),
        title: page.title.clone(),
        section,
        anchor: anchor.into(),
        canonical_url: page.canonical_url.clone(),
        text: text.trim().into(),
        symbols: page.symbols.clone(),
    });
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
    use crate::DocSection;

    #[test]
    fn long_sections_are_bounded_and_overlap() {
        let text = "sentence. ".repeat(1_200);
        let parts = split_bounded(&text);
        assert!(parts.len() > 1);
        assert!(parts
            .iter()
            .all(|part| part.chars().count() <= TARGET_CHARS + 10));
    }

    #[test]
    fn adjacent_short_sections_are_grouped() {
        let page = DocPage {
            path: "example.html".into(),
            title: "Example".into(),
            canonical_url: "https://example.test".into(),
            symbols: Vec::new(),
            sections: (0..20)
                .map(|index| DocSection {
                    heading: format!("Section {index}"),
                    anchor: format!("section-{index}"),
                    text: "Short documentation paragraph.".into(),
                    code: Vec::new(),
                })
                .collect(),
        };
        let chunks = chunk_page(&page, DocsetId::Python, "3.14");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].text.contains("Section 19"));
    }
}
