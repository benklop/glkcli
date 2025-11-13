use ratatui::widgets::BorderType;
use std::env;

/// Determine the appropriate border type based on environment
///
/// This function checks environment variables to determine if we should
/// use plain ASCII borders instead of Unicode box-drawing characters.
/// This is useful for embedded devices, serial consoles, or terminals
/// that don't properly support UTF-8.
///
/// Environment variables checked (in order):
/// - GLKCLI_BORDER_STYLE: "plain" or "unicode" (explicit control)
/// - LC_ALL / LANG: If set to "C" or doesn't contain "UTF", uses plain
///
/// # Returns
///
/// Returns `BorderType::Plain` for ASCII borders or `BorderType::Rounded`
/// for Unicode box-drawing characters.
///
/// # Examples
///
/// ```
/// use glkcli::border_style::get_border_type;
///
/// // Force plain ASCII borders:
/// std::env::set_var("GLKCLI_BORDER_STYLE", "plain");
/// let border = get_border_type();
/// ```
pub fn get_border_type() -> BorderType {
    // Check explicit override first
    if let Ok(style) = env::var("GLKCLI_BORDER_STYLE") {
        return match style.to_lowercase().as_str() {
            "plain" | "ascii" => BorderType::Plain,
            _ => BorderType::Rounded,
        };
    }

    // Check if we're in a C locale (implies ASCII-only)
    if let Ok(lc_all) = env::var("LC_ALL") {
        if lc_all == "C" || lc_all == "POSIX" || !lc_all.to_uppercase().contains("UTF") {
            return BorderType::Plain;
        }
    }

    if let Ok(lang) = env::var("LANG") {
        if lang == "C" || lang == "POSIX" || !lang.to_uppercase().contains("UTF") {
            return BorderType::Plain;
        }
    }

    // Default to Unicode (rounded borders look nicer)
    BorderType::Rounded
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_explicit_plain_border() {
        env::set_var("GLKCLI_BORDER_STYLE", "plain");
        assert!(matches!(get_border_type(), BorderType::Plain));
        env::remove_var("GLKCLI_BORDER_STYLE");
    }

    #[test]
    #[serial]
    fn test_explicit_unicode_border() {
        env::set_var("GLKCLI_BORDER_STYLE", "unicode");
        assert!(matches!(get_border_type(), BorderType::Rounded));
        env::remove_var("GLKCLI_BORDER_STYLE");
    }

    #[test]
    #[serial]
    fn test_c_locale() {
        env::remove_var("GLKCLI_BORDER_STYLE");
        env::set_var("LC_ALL", "C");
        assert!(matches!(get_border_type(), BorderType::Plain));
        env::remove_var("LC_ALL");
    }

    #[test]
    #[serial]
    fn test_utf8_locale() {
        env::remove_var("GLKCLI_BORDER_STYLE");
        env::set_var("LC_ALL", "en_US.UTF-8");
        assert!(matches!(get_border_type(), BorderType::Rounded));
        env::remove_var("LC_ALL");
    }
}
