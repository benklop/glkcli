use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameFormat {
    Unknown,
    ZCode,
    Glulx,
    Tads,
    Hugo,
    Agt,
    Jacl,
    Level9,
    Magnetic,
    Alan2,
    Alan3,
    Adrift,
    Adrift5,
    Scott,
    Plus,
    Taylor,
    Advsys,
}

impl GameFormat {
    pub fn name(&self) -> &'static str {
        match self {
            GameFormat::Unknown => "Unknown",
            GameFormat::ZCode => "Z-code",
            GameFormat::Glulx => "Glulx",
            GameFormat::Tads => "TADS",
            GameFormat::Hugo => "Hugo",
            GameFormat::Agt => "AGT",
            GameFormat::Jacl => "JACL",
            GameFormat::Level9 => "Level 9",
            GameFormat::Magnetic => "Magnetic Scrolls",
            GameFormat::Alan2 => "Alan 2",
            GameFormat::Alan3 => "Alan 3",
            GameFormat::Adrift => "Adrift",
            GameFormat::Adrift5 => "Adrift 5",
            GameFormat::Scott => "Scott Adams",
            GameFormat::Plus => "Plus",
            GameFormat::Taylor => "TaylorMade",
            GameFormat::Advsys => "AdvSys",
        }
    }

    pub fn interpreter(&self) -> Option<&'static str> {
        match self {
            GameFormat::Unknown => None,
            GameFormat::ZCode => Some("bocfel"),
            GameFormat::Glulx => Some("git"),
            GameFormat::Tads => Some("tadsr"),
            GameFormat::Hugo => Some("hugo"),
            GameFormat::Agt => Some("agility"),
            GameFormat::Jacl => Some("jacl"),
            GameFormat::Level9 => Some("level9"),
            GameFormat::Magnetic => Some("magnetic"),
            GameFormat::Alan2 => Some("alan2"),
            GameFormat::Alan3 => Some("alan3"),
            GameFormat::Adrift => Some("scare"),
            GameFormat::Adrift5 => Some("scare"), // Adrift 5 also uses scare
            GameFormat::Scott => Some("scott"),
            GameFormat::Plus => Some("plus"),
            GameFormat::Taylor => Some("taylor"),
            GameFormat::Advsys => Some("advsys"),
        }
    }

    pub fn flags(&self) -> &'static [&'static str] {
        match self {
            // Most interpreters don't need special flags
            _ => &[],
        }
    }
}

impl fmt::Display for GameFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

pub struct MagicPattern {
    pub pattern: &'static [u8],
    pub format: GameFormat,
}

pub const MAGIC_PATTERNS: &[MagicPattern] = &[
    MagicPattern {
        pattern: b"Glul",
        format: GameFormat::Glulx,
    },
    MagicPattern {
        pattern: b"TADS2 bin\x0A\x0D\x1A",
        format: GameFormat::Tads,
    },
    MagicPattern {
        pattern: b"TADS3 r",
        format: GameFormat::Tads,
    },
    MagicPattern {
        pattern: b"Version ",
        format: GameFormat::Adrift,
    },
    MagicPattern {
        pattern: b"\x3C\x42\x3F\xC9",
        format: GameFormat::Adrift5,
    },
    // Z-code is handled specially by version byte validation
];

pub struct ExtensionMapping {
    pub extension: &'static str,
    pub format: GameFormat,
}

pub const EXTENSION_MAPPINGS: &[ExtensionMapping] = &[
    ExtensionMapping { extension: "z1", format: GameFormat::ZCode },
    ExtensionMapping { extension: "z2", format: GameFormat::ZCode },
    ExtensionMapping { extension: "z3", format: GameFormat::ZCode },
    ExtensionMapping { extension: "z4", format: GameFormat::ZCode },
    ExtensionMapping { extension: "z5", format: GameFormat::ZCode },
    ExtensionMapping { extension: "z6", format: GameFormat::ZCode },
    ExtensionMapping { extension: "z7", format: GameFormat::ZCode },
    ExtensionMapping { extension: "z8", format: GameFormat::ZCode },
    ExtensionMapping { extension: "dat", format: GameFormat::ZCode },
    ExtensionMapping { extension: "ulx", format: GameFormat::Glulx },
    ExtensionMapping { extension: "gam", format: GameFormat::Tads },
    ExtensionMapping { extension: "t3", format: GameFormat::Tads },
    ExtensionMapping { extension: "hex", format: GameFormat::Hugo },
    ExtensionMapping { extension: "agx", format: GameFormat::Agt },
    ExtensionMapping { extension: "d$$", format: GameFormat::Agt },
    ExtensionMapping { extension: "jacl", format: GameFormat::Jacl },
    ExtensionMapping { extension: "j2", format: GameFormat::Jacl },
    ExtensionMapping { extension: "l9", format: GameFormat::Level9 },
    ExtensionMapping { extension: "sna", format: GameFormat::Level9 },
    ExtensionMapping { extension: "mag", format: GameFormat::Magnetic },
    ExtensionMapping { extension: "acd", format: GameFormat::Alan2 },
    ExtensionMapping { extension: "a3c", format: GameFormat::Alan3 },
    ExtensionMapping { extension: "taf", format: GameFormat::Adrift },
    ExtensionMapping { extension: "baf", format: GameFormat::Adrift5 },
    ExtensionMapping { extension: "adrift", format: GameFormat::Adrift },
    ExtensionMapping { extension: "saga", format: GameFormat::Scott },
    ExtensionMapping { extension: "plus", format: GameFormat::Plus },
    ExtensionMapping { extension: "tay", format: GameFormat::Taylor },
    ExtensionMapping { extension: "advs", format: GameFormat::Advsys },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_game_format_names() {
        assert_eq!(GameFormat::Unknown.name(), "Unknown");
        assert_eq!(GameFormat::ZCode.name(), "Z-code");
        assert_eq!(GameFormat::Glulx.name(), "Glulx");
        assert_eq!(GameFormat::Tads.name(), "TADS");
        assert_eq!(GameFormat::Hugo.name(), "Hugo");
        assert_eq!(GameFormat::Agt.name(), "AGT");
        assert_eq!(GameFormat::Jacl.name(), "JACL");
        assert_eq!(GameFormat::Level9.name(), "Level 9");
        assert_eq!(GameFormat::Magnetic.name(), "Magnetic Scrolls");
        assert_eq!(GameFormat::Alan2.name(), "Alan 2");
        assert_eq!(GameFormat::Alan3.name(), "Alan 3");
        assert_eq!(GameFormat::Adrift.name(), "Adrift");
        assert_eq!(GameFormat::Adrift5.name(), "Adrift 5");
        assert_eq!(GameFormat::Scott.name(), "Scott Adams");
        assert_eq!(GameFormat::Plus.name(), "Plus");
        assert_eq!(GameFormat::Taylor.name(), "TaylorMade");
        assert_eq!(GameFormat::Advsys.name(), "AdvSys");
    }

    #[test]
    fn test_game_format_interpreters() {
        assert_eq!(GameFormat::Unknown.interpreter(), None);
        assert_eq!(GameFormat::ZCode.interpreter(), Some("bocfel"));
        assert_eq!(GameFormat::Glulx.interpreter(), Some("git"));
        assert_eq!(GameFormat::Tads.interpreter(), Some("tadsr"));
        assert_eq!(GameFormat::Hugo.interpreter(), Some("hugo"));
        assert_eq!(GameFormat::Agt.interpreter(), Some("agility"));
        assert_eq!(GameFormat::Jacl.interpreter(), Some("jacl"));
        assert_eq!(GameFormat::Level9.interpreter(), Some("level9"));
        assert_eq!(GameFormat::Magnetic.interpreter(), Some("magnetic"));
        assert_eq!(GameFormat::Alan2.interpreter(), Some("alan2"));
        assert_eq!(GameFormat::Alan3.interpreter(), Some("alan3"));
        assert_eq!(GameFormat::Adrift.interpreter(), Some("scare"));
        assert_eq!(GameFormat::Adrift5.interpreter(), Some("scare"));
        assert_eq!(GameFormat::Scott.interpreter(), Some("scott"));
        assert_eq!(GameFormat::Plus.interpreter(), Some("plus"));
        assert_eq!(GameFormat::Taylor.interpreter(), Some("taylor"));
        assert_eq!(GameFormat::Advsys.interpreter(), Some("advsys"));
    }

    #[test]
    fn test_game_format_flags() {
        // Currently all formats return empty flags
        assert_eq!(GameFormat::ZCode.flags(), &[] as &[&str]);
        assert_eq!(GameFormat::Glulx.flags(), &[] as &[&str]);
    }

    #[test]
    fn test_game_format_display() {
        assert_eq!(format!("{}", GameFormat::ZCode), "Z-code");
        assert_eq!(format!("{}", GameFormat::Glulx), "Glulx");
        assert_eq!(format!("{}", GameFormat::Unknown), "Unknown");
    }

    #[test]
    fn test_game_format_equality() {
        assert_eq!(GameFormat::ZCode, GameFormat::ZCode);
        assert_ne!(GameFormat::ZCode, GameFormat::Glulx);
        assert_ne!(GameFormat::Unknown, GameFormat::ZCode);
    }

    #[test]
    fn test_game_format_clone_copy() {
        let format1 = GameFormat::ZCode;
        let format2 = format1; // Copy
        assert_eq!(format1, format2);
        
        let format3 = format1.clone();
        assert_eq!(format1, format3);
    }

    #[test]
    fn test_magic_patterns_coverage() {
        // Ensure all magic patterns are valid
        for pattern in MAGIC_PATTERNS {
            assert!(!pattern.pattern.is_empty());
            assert_ne!(pattern.format, GameFormat::Unknown);
        }
    }

    #[test]
    fn test_extension_mappings_coverage() {
        // Ensure all extension mappings are valid
        for mapping in EXTENSION_MAPPINGS {
            assert!(!mapping.extension.is_empty());
            assert_ne!(mapping.format, GameFormat::Unknown);
        }
        
        // Check some specific mappings
        let z5_mapping = EXTENSION_MAPPINGS.iter()
            .find(|m| m.extension == "z5")
            .expect("z5 extension should exist");
        assert_eq!(z5_mapping.format, GameFormat::ZCode);
        
        let ulx_mapping = EXTENSION_MAPPINGS.iter()
            .find(|m| m.extension == "ulx")
            .expect("ulx extension should exist");
        assert_eq!(ulx_mapping.format, GameFormat::Glulx);
    }

    #[test]
    fn test_all_zcode_extensions() {
        for i in 1..=8 {
            let ext = format!("z{}", i);
            let mapping = EXTENSION_MAPPINGS.iter()
                .find(|m| m.extension == ext);
            assert!(mapping.is_some(), "Z-code extension {} should exist", ext);
            assert_eq!(mapping.unwrap().format, GameFormat::ZCode);
        }
    }
}
