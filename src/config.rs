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
    ExtensionMapping { extension: "saga", format: GameFormat::Scott },
    ExtensionMapping { extension: "plus", format: GameFormat::Plus },
    ExtensionMapping { extension: "tay", format: GameFormat::Taylor },
    ExtensionMapping { extension: "advs", format: GameFormat::Advsys },
];
