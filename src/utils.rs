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
}
