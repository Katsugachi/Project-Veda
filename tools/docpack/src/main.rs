use anyhow::{Context, Result};
use clap::Parser;
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use veda_core::DocsetId;
use veda_docs::{DocPack, DocPackManifest, DocPage, DocSection, LicenseInfo};
use walkdir::WalkDir;

#[derive(Debug, Parser)]
#[command(
    name = "veda-docpack",
    about = "Build a deterministic offline Veda documentation pack"
)]
struct Args {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    id: String,
    #[arg(long)]
    name: String,
    #[arg(long)]
    version: String,
    #[arg(long)]
    source_url: String,
    #[arg(long)]
    source_revision: String,
    #[arg(long)]
    canonical_base: String,
    #[arg(long, default_value = "en")]
    locale: String,
    #[arg(long)]
    created_at: String,
    #[arg(long)]
    license_name: String,
    #[arg(long)]
    license_url: String,
    #[arg(long)]
    attribution: String,
    #[arg(long)]
    source_offer_url: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let id = parse_docset(&args.id)?;
    let mut pages = Vec::new();
    for entry in WalkDir::new(&args.input)
        .follow_links(false)
        .sort_by_file_name()
    {
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
        if !matches!(extension.as_str(), "html" | "htm" | "md" | "markdown") {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&args.input)?
            .to_string_lossy()
            .replace('\\', "/");
        let content = fs::read_to_string(entry.path())
            .with_context(|| format!("reading {}", entry.path().display()))?;
        let canonical = format!(
            "{}/{}",
            args.canonical_base.trim_end_matches('/'),
            relative.trim_start_matches('/')
        );
        let page = if matches!(extension.as_str(), "md" | "markdown") {
            parse_markdown(&relative, &canonical, &content)
        } else {
            parse_html(&relative, &canonical, &content)
        };
        if !page.sections.is_empty() {
            pages.push(page);
        }
    }
    let manifest = DocPackManifest {
        schema_version: 1,
        id,
        name: args.name,
        version: args.version,
        source_url: args.source_url,
        source_revision: args.source_revision,
        created_at: args.created_at,
        locale: args.locale,
        page_count: pages.len(),
        license: LicenseInfo {
            name: args.license_name,
            url: args.license_url,
            attribution: args.attribution,
            source_offer_url: args.source_offer_url,
        },
    };
    DocPack { manifest, pages }.write(&args.output)?;
    println!(
        "wrote {} pages to {}",
        DocPack::read(&args.output)?.pages.len(),
        args.output.display()
    );
    Ok(())
}

fn parse_html(path: &str, canonical_url: &str, source: &str) -> DocPage {
    let html = Html::parse_document(source);
    let title_selector = Selector::parse("title, h1").expect("valid selector");
    let title = html
        .select(&title_selector)
        .next()
        .map(text_of)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| title_from_path(path));
    let body_selector = Selector::parse("main, article, body").expect("valid selector");
    let root = html.select(&body_selector).next();
    let mut sections = Vec::<DocSection>::new();
    let mut symbols = BTreeSet::new();
    let symbol_pattern =
        Regex::new(r"(?x)\b(?:[A-Za-z_]\w*(?:::|\.)?){1,5}\b").expect("valid regex");
    if let Some(root) = root {
        for node in root.descendants() {
            let Some(element) = ElementRef::wrap(node) else {
                continue;
            };
            let tag = element.value().name();
            match tag {
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
                            .unwrap()
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
                        sections.last_mut().unwrap().code.push(code);
                    }
                }
                "code" => {
                    let code = element.text().collect::<Vec<_>>().join("");
                    for found in symbol_pattern.find_iter(&code) {
                        let value = found.as_str();
                        if value.len() > 2
                            && (value.contains('.') || value.contains("::") || value.contains('_'))
                        {
                            symbols.insert(value.into());
                        }
                    }
                }
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

fn parse_markdown(path: &str, canonical_url: &str, source: &str) -> DocPage {
    let mut title = title_from_path(path);
    let mut sections = Vec::<DocSection>::new();
    let mut code_fence = false;
    let mut code = String::new();
    for line in source.lines() {
        if line.trim_start().starts_with("```") {
            if code_fence {
                if !code.trim().is_empty() {
                    ensure_section(&mut sections, &title);
                    sections
                        .last_mut()
                        .unwrap()
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
        if let Some(heading) = line
            .trim_start()
            .strip_prefix("# ")
            .or_else(|| line.trim_start().strip_prefix("## "))
            .or_else(|| line.trim_start().strip_prefix("### "))
        {
            let heading = heading.trim().to_string();
            if sections.is_empty() {
                title = heading.clone();
            }
            sections.push(DocSection {
                anchor: slug(&heading),
                heading,
                text: String::new(),
                code: Vec::new(),
            });
        } else if !line.trim().is_empty() {
            ensure_section(&mut sections, &title);
            sections.last_mut().unwrap().text.push_str(line.trim());
            sections.last_mut().unwrap().text.push('\n');
        }
    }
    clean_sections(&mut sections);
    DocPage {
        path: path.into(),
        title,
        canonical_url: canonical_url.into(),
        symbols: Vec::new(),
        sections,
    }
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
fn parse_docset(value: &str) -> Result<DocsetId> {
    match value.to_ascii_lowercase().as_str() {
        "python" => Ok(DocsetId::Python),
        "cpp" | "c++" => Ok(DocsetId::Cpp),
        "html" => Ok(DocsetId::Html),
        "css" => Ok(DocsetId::Css),
        "javascript" | "js" => Ok(DocsetId::Javascript),
        "local" => Ok(DocsetId::Local),
        _ => anyhow::bail!("unknown docset id: {value}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_html_sections_and_symbols() {
        let page = parse_html("task.html", "https://example/task", "<html><head><title>Tasks</title></head><body><main><h2 id='groups'>Task Groups</h2><p>Use structured concurrency.</p><pre><code>asyncio.TaskGroup()</code></pre></main></body></html>");
        assert_eq!(page.title, "Tasks");
        assert_eq!(page.sections[0].anchor, "groups");
        assert!(page.symbols.contains(&"asyncio.TaskGroup".into()));
    }
}
