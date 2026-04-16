/// Check if a char is a Unicode surrogate code point (U+D800..U+DFFF).
fn is_surrogate(ch: char) -> bool {
    matches!(ch as u32, 0xD800..=0xDFFF)
}

/// Sanitize text by removing or replacing problematic Unicode surrogate characters.
///
/// Unicode surrogate code points (U+D800..U+DFFF) are invalid in UTF-8 and can
/// cause API errors with some providers. This function replaces them with the
/// Unicode replacement character (U+FFFD).
pub fn sanitize_surrogates(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for ch in text.chars() {
        if is_surrogate(ch) {
            result.push('\u{FFFD}');
        } else {
            result.push(ch);
        }
    }
    result
}

/// Strip non-ASCII characters from text.
///
/// Useful for endpoints that only support ASCII encoding.
pub fn strip_non_ascii(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii()).collect()
}

/// Clean text for API submission.
///
/// Performs:
/// 1. Surrogate character replacement
/// 2. Null byte removal
/// 3. Excessive whitespace normalization (3+ newlines → 2)
pub fn clean_for_api(text: &str) -> String {
    let sanitized = sanitize_surrogates(text);

    // Remove null bytes
    let no_null: String = sanitized.chars().filter(|c| *c != '\0').collect();

    // Normalize excessive newlines (3+ → 2)
    let mut result = String::with_capacity(no_null.len());
    let mut newline_count = 0;
    for ch in no_null.chars() {
        if ch == '\n' {
            newline_count += 1;
            if newline_count <= 2 {
                result.push(ch);
            }
        } else {
            newline_count = 0;
            result.push(ch);
        }
    }

    result
}

/// Check if a string contains surrogate characters.
pub fn has_surrogates(text: &str) -> bool {
    text.chars().any(|c| is_surrogate(c))
}

/// Check if text is pure ASCII.
pub fn is_ascii(text: &str) -> bool {
    text.is_ascii()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_surrogate_helper() {
        // Valid Unicode scalar values
        assert!(!is_surrogate('A'));
        assert!(!is_surrogate('\u{00E9}'));
        assert!(!is_surrogate('\u{FFFD}')); // replacement char
        assert!(!is_surrogate('\u{D7FF}')); // just below surrogates
        assert!(!is_surrogate('\u{E000}')); // just above surrogates
    }

    #[test]
    fn test_sanitize_surrogates_clean() {
        let text = "Hello, world!";
        assert_eq!(sanitize_surrogates(text), text);
    }

    #[test]
    fn test_sanitize_surrogates_unicode() {
        // Normal unicode characters pass through
        let text = "Héllo 世界 👋";
        assert_eq!(sanitize_surrogates(text), text);
    }

    #[test]
    fn test_sanitize_surrogates_empty() {
        assert_eq!(sanitize_surrogates(""), "");
    }

    #[test]
    fn test_strip_non_ascii_preserves_ascii() {
        assert_eq!(strip_non_ascii("Hello, world!"), "Hello, world!");
    }

    #[test]
    fn test_strip_non_ascii_removes_unicode() {
        assert_eq!(strip_non_ascii("Héllo"), "Hllo");
        assert_eq!(strip_non_ascii("你好"), "");
    }

    #[test]
    fn test_strip_non_ascii_removes_emoji() {
        assert_eq!(strip_non_ascii("Hello 👋"), "Hello ");
    }

    #[test]
    fn test_clean_for_api_removes_null() {
        assert_eq!(clean_for_api("hello\u{0000}world"), "helloworld");
    }

    #[test]
    fn test_clean_for_api_normalizes_newlines() {
        assert_eq!(clean_for_api("a\n\n\n\nb"), "a\n\nb");
    }

    #[test]
    fn test_clean_for_api_preserves_double_newline() {
        assert_eq!(clean_for_api("a\n\nb"), "a\n\nb");
    }

    #[test]
    fn test_clean_for_api_preserves_single_newline() {
        assert_eq!(clean_for_api("a\nb"), "a\nb");
    }

    #[test]
    fn test_has_surrogates_always_false_for_valid_str() {
        // In Rust, valid &str cannot contain surrogate code points
        // (they are not valid Unicode scalar values).
        // This function is a defensive check for external byte sources.
        assert!(!has_surrogates("Hello, world!"));
        assert!(!has_surrogates("Héllo 世界"));
        assert!(!has_surrogates("\u{FFFD}"));
    }

    #[test]
    fn test_is_ascii() {
        assert!(is_ascii("Hello, world!"));
        assert!(!is_ascii("Héllo"));
        assert!(!is_ascii("你好"));
    }

    #[test]
    fn test_clean_for_api_combined() {
        // Null + excess newlines
        let input = "Hello\u{0000}World\n\n\n\nEnd";
        let result = clean_for_api(input);
        assert_eq!(result, "HelloWorld\n\nEnd");
    }
}
