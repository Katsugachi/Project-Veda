use crate::{DocPage, DocSection};
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use std::{collections::BTreeSet, fs, path::Path};
use walkdir::WalkDir;

pub fn ingest_directory(root: &Path, canonical_base: &str) -> Result<Vec<DocPage>, IngestError> {
    let mut pages = Vec::new();
    for entry in WalkDir::new(root).follow_links(false).sort_by_file_name() {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let extension = entry
            .path()
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !is_doc_extension(&extension) {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|_| IngestError::OutsideRoot)?
            .to_string_lossy()
            .replace('\\', "/");
        if relative.starts_with("_sources/") || relative.starts_with("_static/") {
            continue;
        }
        let content = fs::read_to_string(entry.path())?;
        let fallback_canonical = format!(
            "{}/{}",
            canonical_base.trim_end_matches('/'),
            relative.trim_start_matches('/')
        );
        let page = parse_document(
            &relative,
            &fallback_canonical,
            &content,
            canonical_base,
            &extension,
        );
        if !page.sections.is_empty() {
            pages.push(page);
        }
    }
    Ok(pages)
}

/// Ingests an in-memory file list (browser file picker / tests) the same way
/// [`ingest_directory`] walks a folder.
pub fn ingest_memory(
    files: &[(String, String)],
    canonical_base: &str,
) -> Result<Vec<DocPage>, IngestError> {
    let mut pages = Vec::new();
    for (relative, content) in files {
        let relative = relative.replace('\\', "/");
        if relative.split('/').any(|part| part == "..") {
            return Err(IngestError::OutsideRoot);
        }
        let extension = relative
            .rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !is_doc_extension(&extension) {
            continue;
        }
        let fallback_canonical = format!(
            "{}/{}",
            canonical_base.trim_end_matches('/'),
            relative.trim_start_matches('/')
        );
        let page = parse_document(
            &relative,
            &fallback_canonical,
            content,
            canonical_base,
            &extension,
        );
        if !page.sections.is_empty() {
            pages.push(page);
        }
    }
    Ok(pages)
}

fn is_doc_extension(extension: &str) -> bool {
    matches!(
        extension,
        "html" | "htm" | "md" | "markdown" | "txt" | "rst" | "text"
    )
}

fn parse_document(
    relative: &str,
    fallback_canonical: &str,
    content: &str,
    canonical_base: &str,
    extension: &str,
) -> DocPage {
    match extension {
        "md" | "markdown" | "rst" => {
            parse_markdown(relative, fallback_canonical, content, canonical_base)
        }
        "html" | "htm" => parse_html(relative, fallback_canonical, content),
        _ => parse_plaintext(relative, fallback_canonical, content),
    }
}

pub fn parse_plaintext(path: &str, canonical_url: &str, source: &str) -> DocPage {
    let title = title_from_path(path);
    let mut sections = Vec::<DocSection>::new();
    let mut current = String::new();
    for line in source.lines() {
        if line.trim().is_empty() {
            if !current.trim().is_empty() {
                sections.push(DocSection {
                    heading: title.clone(),
                    anchor: slug(&title),
                    text: current.trim().into(),
                    code: Vec::new(),
                });
                current.clear();
            }
            continue;
        }
        current.push_str(line.trim());
        current.push('\n');
    }
    if !current.trim().is_empty() {
        sections.push(DocSection {
            heading: title.clone(),
            anchor: "top".into(),
            text: current.trim().into(),
            code: Vec::new(),
        });
    } else if sections.is_empty() && !source.trim().is_empty() {
        sections.push(DocSection {
            heading: title.clone(),
            anchor: "top".into(),
            text: source.trim().into(),
            code: Vec::new(),
        });
    }
    DocPage {
        path: path.into(),
        title,
        canonical_url: canonical_url.into(),
        symbols: Vec::new(),
        sections,
    }
}

pub fn parse_html(path: &str, canonical_url: &str, source: &str) -> DocPage {
    let html = Html::parse_document(source);
    let title_selector = Selector::parse("title, h1").expect("static selector is valid");
    let title = html
        .select(&title_selector)
        .next()
        .map(text_of)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| title_from_path(path));
    let body_selector = Selector::parse("main, article, body").expect("static selector is valid");
    let root = html.select(&body_selector).next();
    let mut sections = Vec::<DocSection>::new();
    let mut symbols = BTreeSet::new();
    let symbol_pattern = symbol_pattern();
    if let Some(root) = root {
        for node in root.descendants() {
            let Some(element) = ElementRef::wrap(node) else {
                continue;
            };
            match element.value().name() {
                "h1" | "h2" | "h3" | "h4" => {
                    let heading = text_of(element);
                    if !heading.is_empty() {
                        let anchor = element
                            .value()
                            .attr("id")
                            .map(str::to_owned)
                            .unwrap_or_else(|| slug(&heading));
                        sections.push(DocSection {
                            heading,
                            anchor,
                            text: String::new(),
                            code: Vec::new(),
                        });
                    }
                }
                "p" | "li" | "dt" | "dd" | "th" | "td" => {
                    let text = text_of(element);
                    if !text.is_empty() {
                        ensure_section(&mut sections, &title);
                        sections
                            .last_mut()
                            .expect("section exists")
                            .text
                            .push_str(&format!("{text}\n"));
                    }
                }
                "pre" => {
                    let code = element
                        .text()
                        .collect::<Vec<_>>()
                        .join("")
                        .trim()
                        .to_string();
                    if !code.is_empty() {
                        ensure_section(&mut sections, &title);
                        sections.last_mut().expect("section exists").code.push(code);
                    }
                }
                "code" => collect_symbols(
                    &element.text().collect::<Vec<_>>().join(""),
                    &symbol_pattern,
                    &mut symbols,
                ),
                _ => {}
            }
        }
    }
    clean_sections(&mut sections);
    DocPage {
        path: path.into(),
        title,
        canonical_url: canonical_url.into(),
        symbols: symbols.into_iter().take(256).collect(),
        sections,
    }
}

pub fn parse_markdown(
    path: &str,
    fallback_canonical: &str,
    source: &str,
    canonical_base: &str,
) -> DocPage {
    let (frontmatter, markdown) = split_frontmatter(source);
    let mut title =
        frontmatter_value(frontmatter, "title").unwrap_or_else(|| title_from_path(path));
    let slug_value = frontmatter_value(frontmatter, "slug");
    let canonical_url = slug_value.map_or_else(
        || fallback_canonical.to_string(),
        |slug| {
            format!(
                "{}/{}",
                canonical_base.trim_end_matches('/'),
                slug.trim_start_matches('/')
            )
        },
    );
    let mut sections = Vec::<DocSection>::new();
    let mut symbols = BTreeSet::new();
    let symbol_pattern = symbol_pattern();
    let mut code_fence = false;
    let mut code = String::new();
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            if code_fence {
                if !code.trim().is_empty() {
                    ensure_section(&mut sections, &title);
                    collect_symbols(&code, &symbol_pattern, &mut symbols);
                    sections
                        .last_mut()
                        .expect("section exists")
                        .code
                        .push(code.trim_end().into());
                }
                code.clear();
            }
            code_fence = !code_fence;
            continue;
        }
        if code_fence {
            code.push_str(line);
            code.push('\n');
            continue;
        }
        if let Some(heading) = markdown_heading(line) {
            let heading = heading.trim().to_string();
            if sections.is_empty() && title == title_from_path(path) {
                title = heading.clone();
            }
            sections.push(DocSection {
                anchor: slug(&heading),
                heading,
                text: String::new(),
                code: Vec::new(),
            });
        } else if !line.trim().is_empty() && !line.trim().starts_with("{{") {
            ensure_section(&mut sections, &title);
            sections
                .last_mut()
                .expect("section exists")
                .text
                .push_str(line.trim());
            sections.last_mut().expect("section exists").text.push('\n');
            collect_inline_code(line, &symbol_pattern, &mut symbols);
        }
    }
    clean_sections(&mut sections);
    DocPage {
        path: path.into(),
        title,
        canonical_url,
        symbols: symbols.into_iter().take(256).collect(),
        sections,
    }
}

fn split_frontmatter(source: &str) -> (&str, &str) {
    if let Some(rest) = source.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---\n") {
            return (&rest[..end], &rest[end + 5..]);
        }
    }
    ("", source)
}

fn frontmatter_value(frontmatter: &str, key: &str) -> Option<String> {
    frontmatter.lines().find_map(|line| {
        let (found, value) = line.split_once(':')?;
        (found.trim() == key).then(|| value.trim().trim_matches(['\'', '"']).to_string())
    })
}

fn markdown_heading(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    trimmed
        .strip_prefix("#### ")
        .or_else(|| trimmed.strip_prefix("### "))
        .or_else(|| trimmed.strip_prefix("## "))
        .or_else(|| trimmed.strip_prefix("# "))
}

fn collect_inline_code(line: &str, pattern: &Regex, symbols: &mut BTreeSet<String>) {
    for (index, segment) in line.split('`').enumerate() {
        if index % 2 == 1 {
            collect_symbols(segment, pattern, symbols);
        }
    }
}

fn collect_symbols(value: &str, pattern: &Regex, symbols: &mut BTreeSet<String>) {
    for found in pattern.find_iter(value) {
        let value = found.as_str();
        if value.len() > 2 && (value.contains('.') || value.contains("::") || value.contains('_')) {
            symbols.insert(value.into());
        }
    }
}

fn symbol_pattern() -> Regex {
    Regex::new(r"(?x)\b(?:[A-Za-z_]\w*(?:::|\.)?){1,5}\b").expect("static regex is valid")
}

fn text_of(element: ElementRef<'_>) -> String {
    element
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn ensure_section(sections: &mut Vec<DocSection>, title: &str) {
    if sections.is_empty() {
        sections.push(DocSection {
            heading: title.into(),
            anchor: "top".into(),
            text: String::new(),
            code: Vec::new(),
        });
    }
}

fn clean_sections(sections: &mut Vec<DocSection>) {
    for section in sections.iter_mut() {
        section.text = section.text.trim().into();
    }
    sections.retain(|section| !section.text.is_empty() || !section.code.is_empty());
}

fn title_from_path(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Documentation")
        .replace(['-', '_'], " ")
}

fn slug(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Walk(#[from] walkdir::Error),
    #[error("documentation input escaped its root directory")]
    OutsideRoot,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mdn_frontmatter_builds_canonical_url() {
        let page = parse_markdown(
            "index.md",
            "fallback",
            "---\ntitle: Array.prototype.map()\nslug: Web/JavaScript/Reference/Global_Objects/Array/map\n---\n\n## Syntax\nUse `Array.prototype.map()`.",
            "https://developer.mozilla.org/en-US/docs",
        );
        assert_eq!(page.title, "Array.prototype.map()");
        assert!(page
            .canonical_url
            .ends_with("Web/JavaScript/Reference/Global_Objects/Array/map"));
        assert!(page.symbols.contains(&"Array.prototype.map".into()));
    }

    #[test]
    fn memory_ingest_accepts_markdown_and_plain_text() {
        let pages = ingest_memory(
            &[
                (
                    "notes/intro.md".into(),
                    "# Hello\n\nThis is a local note about `std::vector`.".into(),
                ),
                (
                    "notes/readme.txt".into(),
                    "Plain text documentation for the project.\n\nSecond paragraph.".into(),
                ),
                ("notes/skip.bin".into(), "not documentation".into()),
            ],
            "veda://local/notes",
        )
        .unwrap();
        assert_eq!(pages.len(), 2);
        assert!(pages.iter().any(|page| page.title.contains("Hello")));
    }

    #[test]
    fn memory_ingest_rejects_parent_directory_escape() {
        let error = ingest_memory(
            &[("../secret.md".into(), "# no".into())],
            "veda://local/notes",
        )
        .unwrap_err();
        assert!(matches!(error, IngestError::OutsideRoot));
    }
}
