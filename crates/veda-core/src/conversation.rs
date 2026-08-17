//! Small-talk detection for the conversational fast path.
//!
//! A greeting like "hi" must not pay for a search-planning round trip and a
//! documentation lookup — but misclassifying a real question as small talk
//! would silently drop retrieval, which is worse. This classifier is therefore
//! deliberately conservative: it only matches an explicit allow-list of
//! greetings and pleasantries, and any input that looks even slightly like
//! code or a technical question is always routed through retrieval.

/// True when `message` is small talk (a greeting, thanks, goodbye, or a simple
/// question about Veda itself) that needs no documentation search.
pub fn is_conversational(message: &str) -> bool {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return true;
    }
    // Code, punctuation-heavy symbols and technical phrasing always take the
    // retrieval path, no matter how short they are.
    if looks_technical(trimmed) {
        return false;
    }
    let normalized = normalize(trimmed);

    const GREETINGS: &[&str] = &[
        "hi",
        "hi there",
        "hello",
        "hello there",
        "hello world",
        "hey",
        "hey there",
        "yo",
        "hola",
        "howdy",
        "sup",
        "greetings",
        "good morning",
        "good afternoon",
        "good evening",
        "good night",
        "how are you",
        "how are you doing",
        "how are you today",
        "hows it going",
        "how's it going",
        "how is it going",
        "whats up",
        "what's up",
        "what is up",
        "are you there",
        "are you real",
        "who are you",
        "what are you",
        "what can you do",
        "what do you do",
        "what can you help with",
        "what can you help me with",
        "can you help",
        "can you help me",
        "help",
        "thanks",
        "thank you",
        "thanks a lot",
        "thank you so much",
        "thx",
        "ty",
        "bye",
        "goodbye",
        "see you",
        "see ya",
        "ok",
        "okay",
        "nice",
        "cool",
        "great",
        "lol",
        "test",
        "testing",
    ];
    GREETINGS.contains(&normalized.as_str())
}

/// Normalizes a message for allow-list matching: lower-cased, with punctuation
/// (except apostrophes and internal hyphens) removed and whitespace collapsed.
fn normalize(message: &str) -> String {
    let lower = message.to_lowercase();
    let filtered = lower
        .chars()
        .filter(|character| !character.is_ascii_punctuation() || matches!(character, '\'' | '-'))
        .collect::<String>();
    filtered.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Heuristic guard: anything carrying code-ish markers is never small talk.
fn looks_technical(message: &str) -> bool {
    message.contains('`')
        || message.contains('{')
        || message.contains('}')
        || message.contains(';')
        || message.contains('(')
        || message.contains(')')
        || message.contains('=')
        || message.contains('[')
        || message.contains(']')
        || message.contains('\n')
        || message.contains("import ")
        || message.contains("def ")
        || message.contains("print")
        || message.contains('#')
}

#[cfg(test)]
mod tests {
    use super::is_conversational;

    #[test]
    fn greetings_are_conversational() {
        for message in [
            "hi",
            "hi!",
            "Hello",
            "hey there",
            "good morning",
            "how are you?",
            "what's up",
            "thanks",
            "thank you so much",
            "bye",
            "ok",
        ] {
            assert!(
                is_conversational(message),
                "{message:?} should be conversational"
            );
        }
    }

    #[test]
    fn technical_questions_never_take_the_fast_path() {
        for message in [
            "how do I reverse a list in python?",
            "python list append",
            "std::vector move constructor",
            "what does asyncio.TaskGroup do",
            "print(1)",
            "def f(): return 1",
            "import os",
            "how do I sort a dict by value",
        ] {
            assert!(
                !is_conversational(message),
                "{message:?} must use retrieval"
            );
        }
    }

    #[test]
    fn empty_messages_are_conversational() {
        assert!(is_conversational(""));
        assert!(is_conversational("   "));
    }
}
