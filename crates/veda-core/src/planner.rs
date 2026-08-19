//! Deterministic search planner.
//!
//! The 1B chat model used to spend a full generation just to emit a JSON plan
//! before any retrieval ran. On a GPU that extra round-trip is still hundreds
//! of milliseconds of prompt processing plus whatever tokens the model
//! rambles; on a loaded machine it was a large fraction of the "four minutes
//! per answer" complaint. Planning is a string problem — extract symbols,
//! pick the relevant packs, keep the user's wording as the query — so Rust
//! does it in microseconds and the model is reserved for the actual answer.

use crate::{DocsetId, SearchQueryPlan};

/// Builds a bounded search plan from the user's question and the packs that
/// are actually in scope. Never talks to the model.
pub fn plan_from_question(message: &str, available: &[DocsetId]) -> SearchQueryPlan {
    let query: String = message.trim().chars().take(240).collect();
    let symbols = extract_symbols(message);
    let inferred = infer_docsets(message);
    let mut docsets = if inferred.is_empty() {
        available.to_vec()
    } else {
        inferred
            .into_iter()
            .filter(|docset| available.is_empty() || available.contains(docset))
            .collect()
    };
    // User-imported libraries have no language cue. Dropping them whenever
    // the question mentions Python/C++/... made "Add your own docs" a no-op
    // at search time.
    if available.contains(&DocsetId::Local) && !docsets.contains(&DocsetId::Local) {
        docsets.push(DocsetId::Local);
    }
    let mut queries = Vec::new();
    if !query.is_empty() {
        queries.push(query);
    }
    // A second, identifier-only query helps BM25 when the user buried the
    // symbol in a long sentence.
    if let Some(symbol) = symbols.first() {
        let already_named = queries.first().is_some_and(|query| {
            query
                .to_ascii_lowercase()
                .contains(&symbol.to_ascii_lowercase())
        });
        if !already_named {
            queries.push(symbol.clone());
        }
    }
    SearchQueryPlan {
        queries,
        docsets,
        symbols,
        result_count: 8,
    }
    .sanitize()
}

/// Pulls language-looking identifiers (`asyncio.TaskGroup`, `std::vector`,
/// `grid-template-columns`) out of free text.
pub fn extract_symbols(message: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut current = String::new();
    let flush = |symbols: &mut Vec<String>, current: &mut String| {
        if current.len() > 2
            && (current.contains('.') || current.contains("::") || current.contains('_'))
            && !symbols.contains(current)
        {
            symbols.push(std::mem::take(current));
        } else {
            current.clear();
        }
    };
    for character in message.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | ':' | '.' | '#' | '-') {
            current.push(character);
        } else {
            flush(&mut symbols, &mut current);
        }
    }
    flush(&mut symbols, &mut current);
    symbols.truncate(12);
    symbols
}

/// Maps obvious language cues onto the official packs. Returns an empty list
/// when the question is ambiguous so the caller searches everything in scope.
pub fn infer_docsets(message: &str) -> Vec<DocsetId> {
    let lower = message.to_ascii_lowercase();
    let mut found = Vec::new();
    let push = |found: &mut Vec<DocsetId>, id: DocsetId| {
        if !found.contains(&id) {
            found.push(id);
        }
    };
    if contains_any(
        &lower,
        &[
            "python", "asyncio", "django", "pytest", "pip ", "def ", "pypi", "cpython", "numpy",
            "pandas",
        ],
    ) {
        push(&mut found, DocsetId::Python);
    }
    if contains_any(
        &lower,
        &[
            "c++",
            "cpp",
            "std::",
            "nullptr",
            "cmake",
            "template<",
            "cstdio",
            "cppreference",
            "clang",
        ],
    ) {
        push(&mut found, DocsetId::Cpp);
    }
    if contains_any(
        &lower,
        &[
            "html",
            "<div",
            "<span",
            "aria-",
            "doctype",
            "semantic element",
        ],
    ) {
        push(&mut found, DocsetId::Html);
    }
    if contains_any(
        &lower,
        &[
            "css",
            "flexbox",
            "grid-template",
            "stylesheet",
            "media query",
            "padding:",
            "margin:",
        ],
    ) {
        push(&mut found, DocsetId::Css);
    }
    if contains_any(
        &lower,
        &[
            "javascript",
            "typescript",
            "ecmascript",
            "promise",
            "node.js",
            "nodejs",
            ".then(",
            "addEventListener",
        ],
    ) {
        push(&mut found, DocsetId::Javascript);
    }
    found
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planner_is_instant_and_keeps_the_question() {
        let plan = plan_from_question(
            "how does asyncio.TaskGroup cancel sibling tasks?",
            &[DocsetId::Python, DocsetId::Cpp],
        );
        assert_eq!(
            plan.queries[0],
            "how does asyncio.TaskGroup cancel sibling tasks?"
        );
        assert!(plan
            .symbols
            .iter()
            .any(|symbol| symbol.contains("TaskGroup")));
        assert_eq!(plan.docsets, vec![DocsetId::Python]);
        assert!(plan.result_count >= 4 && plan.result_count <= 16);
    }

    #[test]
    fn ambiguous_questions_search_everything_in_scope() {
        let plan = plan_from_question(
            "how do I reverse a list?",
            &[DocsetId::Python, DocsetId::Javascript],
        );
        assert_eq!(plan.docsets, vec![DocsetId::Javascript, DocsetId::Python]);
    }

    #[test]
    fn cpp_and_css_cues_are_detected() {
        assert_eq!(
            infer_docsets("std::vector move constructor"),
            vec![DocsetId::Cpp]
        );
        assert_eq!(
            infer_docsets("css flexbox vs grid-template"),
            vec![DocsetId::Css]
        );
    }

    #[test]
    fn symbols_split_out_of_prose() {
        let symbols =
            extract_symbols("Please explain std::vector::push_back and Array.prototype.map");
        assert!(symbols.iter().any(|symbol| symbol.contains("push_back")));
        assert!(symbols
            .iter()
            .any(|symbol| symbol.contains("prototype.map")));
    }

    #[test]
    fn local_libraries_stay_in_scope_when_a_language_is_inferred() {
        let plan = plan_from_question(
            "how do I reverse a list in python?",
            &[DocsetId::Python, DocsetId::Local],
        );
        assert!(plan.docsets.contains(&DocsetId::Python));
        assert!(
            plan.docsets.contains(&DocsetId::Local),
            "user docs must still be searched: {:?}",
            plan.docsets
        );
    }

    #[test]
    fn empty_input_does_not_panic() {
        let plan = plan_from_question("   ", &[]);
        assert!(plan.queries.is_empty() || plan.queries.iter().all(|query| query.is_empty()));
    }
}
