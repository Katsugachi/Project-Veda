//! User-facing error cleanup.
//!
//! Backend failures must never surface raw OS noise such as
//! "No such file or directory (os error 2)" or reqwest's full request URLs
//! and connect diagnostics. This module turns those into short messages that
//! are safe and useful to show in the UI.

/// Strips raw OS error suffixes and transport noise from an error string so
/// the result is short, stable and actionable in the UI.
pub fn friendly_error(message: &str) -> String {
    let mut text = message.trim().to_string();

    // reqwest prefixes transport failures with the full request URL:
    // "error sending request for url (https://…): <inner>". Keep <inner>.
    if let Some(rest) = text.strip_prefix("error sending request for url (") {
        if let Some(separator) = rest.find("): ") {
            text = rest[separator + 3..].trim().to_string();
        }
    }

    // reqwest hides the useful part behind "client error (Connect)".
    for (pattern, replacement) in [
        (
            "client error (Connect) for url",
            "could not connect to the download server",
        ),
        (
            "client error (Connect)",
            "could not connect to the download server",
        ),
        ("builder error", "invalid client configuration"),
    ] {
        if let Some(rest) = text.strip_prefix(pattern) {
            let rest = rest.trim_start_matches(['(', ' ', ':']).to_string();
            let mut updated = replacement.to_string();
            if !rest.is_empty() {
                updated.push(' ');
                updated.push_str(&rest);
            }
            text = updated;
        }
    }

    // Strip trailing "(os error N)" chunks, which Rust appends to IO errors.
    loop {
        let Some(index) = text.rfind("(os error") else {
            break;
        };
        text.truncate(index);
        text = text
            .trim_end_matches([' ', ':', ';', ',', '.', '(', '-'])
            .to_string();
    }

    if text.is_empty() {
        "the operating system reported an I/O failure".to_string()
    } else {
        text
    }
}

/// Adds a short context to an error while keeping the message friendly.
pub fn contextual_error(context: &str, error: &dyn std::fmt::Display) -> String {
    let detail = friendly_error(&error.to_string());
    if detail.is_empty() || detail.eq_ignore_ascii_case(context) {
        context.to_string()
    } else {
        format!("{context}: {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_os_error_suffixes() {
        assert_eq!(
            friendly_error("No such file or directory (os error 2)"),
            "No such file or directory"
        );
        assert_eq!(
            friendly_error("The system cannot find the path specified. (os error 3)"),
            "The system cannot find the path specified"
        );
    }

    #[test]
    fn strips_reqwest_request_prefix() {
        assert_eq!(
            friendly_error(
                "error sending request for url (https://huggingface.co/x.gguf): operation timed out"
            ),
            "operation timed out"
        );
    }

    #[test]
    fn keeps_plain_messages_untouched() {
        assert_eq!(
            friendly_error("download server returned 404 Not Found"),
            "download server returned 404 Not Found"
        );
    }

    #[test]
    fn never_returns_empty_or_raw_os_text() {
        assert!(!friendly_error("(os error 2)").is_empty());
        assert!(!friendly_error("(os error 2)").contains("(os error"));
        assert!(!friendly_error("error sending request for url (x): ").is_empty());
    }

    #[test]
    fn context_is_added_once() {
        assert_eq!(
            contextual_error(
                "Could not unpack the runtime",
                &std::io::Error::from_raw_os_error(2)
            ),
            "Could not unpack the runtime: No such file or directory"
        );
    }
}
