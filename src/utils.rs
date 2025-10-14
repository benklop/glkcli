//! Utility functions
//!
//! This module contains pure helper functions that don't depend on application state.
//! These are general-purpose utilities that can be used across the application.

/// Decode common HTML entities in text
///
/// This function takes a string that may contain HTML entities (like `&#039;`, `&quot;`, etc.)
/// and returns a string with those entities decoded to their Unicode equivalents.
///
/// # Examples
///
/// ```
/// use glkcli_rust::utils::decode_html_entities;
///
/// let encoded = "It&#039;s a beautiful day";
/// let decoded = decode_html_entities(encoded);
/// assert_eq!(decoded, "It's a beautiful day");
/// ```
pub fn decode_html_entities(text: &str) -> String {
    html_escape::decode_html_entities(text).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_basic_entities() {
        assert_eq!(decode_html_entities("&#039;"), "'");
        assert_eq!(decode_html_entities("&quot;"), "\"");
        assert_eq!(decode_html_entities("&amp;"), "&");
    }

    #[test]
    fn test_decode_in_context() {
        let input = "It&#039;s a &quot;great&quot; day!";
        let expected = "It's a \"great\" day!";
        assert_eq!(decode_html_entities(input), expected);
    }

    #[test]
    fn test_decode_no_entities() {
        let input = "Plain text with no entities";
        assert_eq!(decode_html_entities(input), input);
    }

    #[test]
    fn test_decode_multiple_amp() {
        let input = "&amp;&amp;&amp;";
        let expected = "&&&";
        assert_eq!(decode_html_entities(input), expected);
    }

    #[test]
    fn test_decode_mixed_entities() {
        let input = "She said &quot;Hello!&quot; &amp; waved goodbye.";
        let expected = "She said \"Hello!\" & waved goodbye.";
        assert_eq!(decode_html_entities(input), expected);
    }

    #[test]
    fn test_decode_apostrophe_variations() {
        assert_eq!(decode_html_entities("&#39;"), "'");
        assert_eq!(decode_html_entities("&#x27;"), "'");
        assert_eq!(decode_html_entities("&apos;"), "'");
    }

    #[test]
    fn test_decode_empty_string() {
        assert_eq!(decode_html_entities(""), "");
    }

    #[test]
    fn test_decode_only_entities() {
        assert_eq!(decode_html_entities("&quot;&amp;&#039;"), "\"&'");
    }

    #[test]
    fn test_decode_incomplete_entity() {
        // html-escape crate should handle these gracefully
        let input = "&quot";
        let result = decode_html_entities(input);
        // The exact behavior depends on html-escape, but it should handle it
        assert!(!result.is_empty());
    }

    #[test]
    fn test_decode_text_with_gt_lt() {
        let input = "&lt;tag&gt; &amp; more";
        let expected = "<tag> & more";
        assert_eq!(decode_html_entities(input), expected);
    }
}
