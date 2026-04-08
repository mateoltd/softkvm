use serde::{Deserialize, Serialize};

/// operating system type, used to determine keyboard remapping rules
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OsType {
    Windows,
    #[serde(alias = "macos")]
    MacOS,
    Linux,
}

impl std::fmt::Display for OsType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Windows => write!(f, "windows"),
            Self::MacOS => write!(f, "macos"),
            Self::Linux => write!(f, "linux"),
        }
    }
}

/// a keyboard shortcut translation between OS pairs
/// these go beyond simple modifier swaps and handle combos
/// that have fundamentally different key sequences per OS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutTranslation {
    /// human-readable intent (e.g., "app_switcher", "quit_app")
    pub intent: String,
    /// mac shortcut string (e.g., "meta+tab")
    pub mac: String,
    /// windows shortcut string (e.g., "alt+tab")
    pub windows: String,
}

/// keyboard remapping configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardConfig {
    /// enable automatic modifier translation based on OS pair
    #[serde(default = "default_true")]
    pub auto_remap: bool,

    /// combo-specific translations that go beyond modifier swaps
    #[serde(default)]
    pub translations: Vec<ShortcutTranslation>,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            auto_remap: true,
            translations: default_translations(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// built-in shortcut translations for common Mac <-> Windows differences
pub fn default_translations() -> Vec<ShortcutTranslation> {
    vec![
        ShortcutTranslation {
            intent: "app_switcher".into(),
            mac: "meta+tab".into(),
            windows: "alt+tab".into(),
        },
        ShortcutTranslation {
            intent: "quit_app".into(),
            mac: "meta+q".into(),
            windows: "alt+F4".into(),
        },
        ShortcutTranslation {
            intent: "search".into(),
            mac: "meta+space".into(),
            windows: "super+s".into(),
        },
        ShortcutTranslation {
            intent: "screenshot".into(),
            mac: "meta+shift+4".into(),
            windows: "super+shift+s".into(),
        },
        ShortcutTranslation {
            intent: "lock_screen".into(),
            mac: "meta+ctrl+q".into(),
            windows: "super+l".into(),
        },
    ]
}

/// Deskflow modifier mapping for a given OS pair
/// returns (modifier_name, mapped_to) pairs for the Deskflow config
pub fn deskflow_modifier_mapping(server_os: OsType, client_os: OsType) -> Vec<(&'static str, &'static str)> {
    match (server_os, client_os) {
        // Mac server, Windows client: Mac keyboard controlling Windows
        (OsType::MacOS, OsType::Windows) => vec![
            ("meta", "ctrl"),
            ("ctrl", "super"),
        ],
        // Windows server, Mac client: Windows keyboard controlling Mac
        (OsType::Windows, OsType::MacOS) => vec![
            ("ctrl", "meta"),
            ("super", "ctrl"),
        ],
        _ => vec![],
    }
}

// --- key interceptor types ---

/// modifier keys we track for combo matching
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Meta,   // Cmd on Mac, Win on Windows
    Super,  // same as Meta in most contexts
}

/// a parsed key combo (e.g., "meta+tab" -> KeyCombo { modifiers: [Meta], key: "tab" })
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCombo {
    pub modifiers: Vec<Modifier>,
    pub key: String,
}

impl KeyCombo {
    /// parse a shortcut string like "meta+shift+4" or "alt+F4"
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('+').collect();
        if parts.is_empty() {
            return None;
        }

        let mut modifiers = Vec::new();
        let mut key = None;

        for (i, part) in parts.iter().enumerate() {
            let lower = part.to_lowercase();
            match lower.as_str() {
                "ctrl" | "control" => modifiers.push(Modifier::Ctrl),
                "alt" | "option" => modifiers.push(Modifier::Alt),
                "shift" => modifiers.push(Modifier::Shift),
                "meta" | "cmd" | "command" => modifiers.push(Modifier::Meta),
                "super" | "win" | "windows" => modifiers.push(Modifier::Super),
                _ => {
                    // last non-modifier part is the key
                    if i == parts.len() - 1 {
                        key = Some(part.to_string());
                    } else {
                        return None; // non-modifier in middle position
                    }
                }
            }
        }

        Some(KeyCombo {
            modifiers,
            key: key.unwrap_or_default(),
        })
    }
}

/// a resolved translation rule: when this combo is pressed, synthesize the other
#[derive(Debug, Clone)]
pub struct TranslationRule {
    pub intent: String,
    pub from: KeyCombo,
    pub to: KeyCombo,
}

/// build translation rules for a given OS pair direction
/// if local_os is Mac and remote_os is Windows, we translate Mac combos to Windows combos
pub fn build_translation_rules(
    local_os: OsType,
    remote_os: OsType,
    translations: &[ShortcutTranslation],
) -> Vec<TranslationRule> {
    translations
        .iter()
        .filter_map(|t| {
            let (from_str, to_str) = match (local_os, remote_os) {
                (OsType::MacOS, OsType::Windows) => (t.mac.as_str(), t.windows.as_str()),
                (OsType::Windows, OsType::MacOS) => (t.windows.as_str(), t.mac.as_str()),
                _ => return None,
            };

            let from = KeyCombo::parse(from_str)?;
            let to = KeyCombo::parse(to_str)?;
            Some(TranslationRule {
                intent: t.intent.clone(),
                from,
                to,
            })
        })
        .collect()
}

/// look up whether a key combo matches any translation rule
pub fn find_translation<'a>(
    combo: &KeyCombo,
    rules: &'a [TranslationRule],
) -> Option<&'a TranslationRule> {
    rules.iter().find(|rule| {
        rule.from.key.eq_ignore_ascii_case(&combo.key)
            && rule.from.modifiers.len() == combo.modifiers.len()
            && rule.from.modifiers.iter().all(|m| combo.modifiers.contains(m))
    })
}

// --- platform key code mappings ---
// pure functions mapping between OS key codes and our Modifier/KeyCombo types.
// these don't call any OS APIs, so they compile and test on any platform.

// -- Windows Virtual Key codes --

/// extract Modifier list from a set of (vk_code, is_pressed) pairs
pub fn modifiers_from_vk_state(modifier_states: &[(u32, bool)]) -> Vec<Modifier> {
    let mut mods = Vec::new();
    for &(vk, pressed) in modifier_states {
        if !pressed {
            continue;
        }
        match vk {
            0xA2 | 0xA3 => mods.push(Modifier::Ctrl),  // VK_LCONTROL, VK_RCONTROL
            0xA4 | 0xA5 => mods.push(Modifier::Alt),    // VK_LMENU, VK_RMENU
            0xA0 | 0xA1 => mods.push(Modifier::Shift),  // VK_LSHIFT, VK_RSHIFT
            0x5B | 0x5C => mods.push(Modifier::Meta),    // VK_LWIN, VK_RWIN
            _ => {}
        }
    }
    mods.dedup();
    mods
}

/// map a Windows VK code to a key name string
pub fn key_name_from_vk(vk: u32) -> Option<String> {
    let name = match vk {
        0x09 => "tab",
        0x0D => "return",
        0x1B => "escape",
        0x20 => "space",
        0x2E => "delete",
        0x08 => "backspace",
        // arrow keys
        0x25 => "left",
        0x26 => "up",
        0x27 => "right",
        0x28 => "down",
        // A-Z
        v @ 0x41..=0x5A => {
            return Some(((v as u8 - 0x41 + b'a') as char).to_string());
        }
        // 0-9
        v @ 0x30..=0x39 => {
            return Some(((v as u8 - 0x30 + b'0') as char).to_string());
        }
        // F1-F12
        v @ 0x70..=0x7B => {
            return Some(format!("F{}", v - 0x70 + 1));
        }
        _ => return None,
    };
    Some(name.into())
}

/// build a KeyCombo from a VK code and modifier state pairs
pub fn combo_from_vk(vk_code: u32, modifier_states: &[(u32, bool)]) -> Option<KeyCombo> {
    let key = key_name_from_vk(vk_code)?;
    let modifiers = modifiers_from_vk_state(modifier_states);
    Some(KeyCombo { modifiers, key })
}

/// map a Modifier to its Windows VK code (left variant)
pub fn modifier_to_vk(m: &Modifier) -> u32 {
    match m {
        Modifier::Ctrl => 0xA2,   // VK_LCONTROL
        Modifier::Alt => 0xA4,    // VK_LMENU
        Modifier::Shift => 0xA0,  // VK_LSHIFT
        Modifier::Meta | Modifier::Super => 0x5B, // VK_LWIN
    }
}

/// map a key name back to a Windows VK code
pub fn key_name_to_vk(name: &str) -> Option<u32> {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "tab" => Some(0x09),
        "return" | "enter" => Some(0x0D),
        "escape" | "esc" => Some(0x1B),
        "space" => Some(0x20),
        "delete" | "del" => Some(0x2E),
        "backspace" => Some(0x08),
        "left" => Some(0x25),
        "up" => Some(0x26),
        "right" => Some(0x27),
        "down" => Some(0x28),
        s if s.len() == 1 => {
            let ch = s.chars().next()?;
            if ch.is_ascii_lowercase() {
                Some((ch as u32 - 'a' as u32) + 0x41)
            } else if ch.is_ascii_digit() {
                Some((ch as u32 - '0' as u32) + 0x30)
            } else {
                None
            }
        }
        s if s.starts_with('f') || s.starts_with('F') => {
            let num: u32 = s[1..].parse().ok()?;
            if (1..=12).contains(&num) {
                Some(0x70 + num - 1)
            } else {
                None
            }
        }
        _ => None,
    }
}

// -- macOS CoreGraphics key codes --

/// extract Modifier list from a macOS CGEventFlags bitmask
pub fn modifiers_from_cg_flags(flags: u64) -> Vec<Modifier> {
    let mut mods = Vec::new();
    if flags & 0x00100000 != 0 { mods.push(Modifier::Meta); }   // kCGEventFlagMaskCommand
    if flags & 0x00040000 != 0 { mods.push(Modifier::Ctrl); }   // kCGEventFlagMaskControl
    if flags & 0x00080000 != 0 { mods.push(Modifier::Alt); }    // kCGEventFlagMaskAlternate
    if flags & 0x00020000 != 0 { mods.push(Modifier::Shift); }  // kCGEventFlagMaskShift
    mods
}

/// map a macOS keycode to a key name string
/// macOS keycodes are position-based, not character-based
pub fn key_name_from_cg_keycode(keycode: u16) -> Option<String> {
    let name = match keycode {
        0x30 => "tab",
        0x24 => "return",
        0x35 => "escape",
        0x31 => "space",
        0x33 => "backspace",
        0x75 => "delete",
        // arrow keys
        0x7B => "left",
        0x7E => "up",
        0x7C => "right",
        0x7D => "down",
        // ANSI letter keys (position-based layout)
        0x00 => "a",
        0x0B => "b",
        0x08 => "c",
        0x02 => "d",
        0x0E => "e",
        0x03 => "f",
        0x05 => "g",
        0x04 => "h",
        0x22 => "i",
        0x26 => "j",
        0x28 => "k",
        0x25 => "l",
        0x2E => "m",
        0x2D => "n",
        0x1F => "o",
        0x23 => "p",
        0x0C => "q",
        0x0F => "r",
        0x01 => "s",
        0x11 => "t",
        0x20 => "u",
        0x09 => "v",
        0x0D => "w",
        0x07 => "x",
        0x10 => "y",
        0x06 => "z",
        // number keys
        0x1D => "0",
        0x12 => "1",
        0x13 => "2",
        0x14 => "3",
        0x15 => "4",
        0x17 => "5",
        0x16 => "6",
        0x1A => "7",
        0x1C => "8",
        0x19 => "9",
        // F keys
        0x7A => "F1",
        0x78 => "F2",
        0x63 => "F3",
        0x76 => "F4",
        0x60 => "F5",
        0x61 => "F6",
        0x62 => "F7",
        0x64 => "F8",
        0x65 => "F9",
        0x6D => "F10",
        0x67 => "F11",
        0x6F => "F12",
        _ => return None,
    };
    Some(name.into())
}

/// build a KeyCombo from a macOS keycode and CGEventFlags
pub fn combo_from_cg(keycode: u16, flags: u64) -> Option<KeyCombo> {
    let key = key_name_from_cg_keycode(keycode)?;
    let modifiers = modifiers_from_cg_flags(flags);
    Some(KeyCombo { modifiers, key })
}

/// map a Modifier to a macOS CGEventFlags bit
pub fn modifier_to_cg_flag(m: &Modifier) -> u64 {
    match m {
        Modifier::Meta | Modifier::Super => 0x00100000, // kCGEventFlagMaskCommand
        Modifier::Ctrl => 0x00040000,                   // kCGEventFlagMaskControl
        Modifier::Alt => 0x00080000,                    // kCGEventFlagMaskAlternate
        Modifier::Shift => 0x00020000,                  // kCGEventFlagMaskShift
    }
}

/// map a key name to a macOS keycode
pub fn key_name_to_cg_keycode(name: &str) -> Option<u16> {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "tab" => Some(0x30),
        "return" | "enter" => Some(0x24),
        "escape" | "esc" => Some(0x35),
        "space" => Some(0x31),
        "backspace" => Some(0x33),
        "delete" | "del" => Some(0x75),
        "left" => Some(0x7B),
        "up" => Some(0x7E),
        "right" => Some(0x7C),
        "down" => Some(0x7D),
        "a" => Some(0x00),
        "b" => Some(0x0B),
        "c" => Some(0x08),
        "d" => Some(0x02),
        "e" => Some(0x0E),
        "f" => Some(0x03),
        "g" => Some(0x05),
        "h" => Some(0x04),
        "i" => Some(0x22),
        "j" => Some(0x26),
        "k" => Some(0x28),
        "l" => Some(0x25),
        "m" => Some(0x2E),
        "n" => Some(0x2D),
        "o" => Some(0x1F),
        "p" => Some(0x23),
        "q" => Some(0x0C),
        "r" => Some(0x0F),
        "s" => Some(0x01),
        "t" => Some(0x11),
        "u" => Some(0x20),
        "v" => Some(0x09),
        "w" => Some(0x0D),
        "x" => Some(0x07),
        "y" => Some(0x10),
        "z" => Some(0x06),
        "0" => Some(0x1D),
        "1" => Some(0x12),
        "2" => Some(0x13),
        "3" => Some(0x14),
        "4" => Some(0x15),
        "5" => Some(0x17),
        "6" => Some(0x16),
        "7" => Some(0x1A),
        "8" => Some(0x1C),
        "9" => Some(0x19),
        s if s.starts_with('f') => {
            let num: u16 = s[1..].parse().ok()?;
            match num {
                1 => Some(0x7A),
                2 => Some(0x78),
                3 => Some(0x63),
                4 => Some(0x76),
                5 => Some(0x60),
                6 => Some(0x61),
                7 => Some(0x62),
                8 => Some(0x64),
                9 => Some(0x65),
                10 => Some(0x6D),
                11 => Some(0x67),
                12 => Some(0x6F),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_translations() {
        let translations = default_translations();
        assert!(translations.len() >= 5);
        assert_eq!(translations[0].intent, "app_switcher");
    }

    #[test]
    fn test_modifier_mapping_mac_to_windows() {
        let mapping = deskflow_modifier_mapping(OsType::MacOS, OsType::Windows);
        assert_eq!(mapping.len(), 2);
        assert!(mapping.contains(&("meta", "ctrl")));
        assert!(mapping.contains(&("ctrl", "super")));
    }

    #[test]
    fn test_modifier_mapping_same_os() {
        let mapping = deskflow_modifier_mapping(OsType::Windows, OsType::Windows);
        assert!(mapping.is_empty());
    }

    #[test]
    fn test_parse_key_combo() {
        let combo = KeyCombo::parse("meta+tab").unwrap();
        assert_eq!(combo.modifiers, vec![Modifier::Meta]);
        assert_eq!(combo.key, "tab");
    }

    #[test]
    fn test_parse_multi_modifier() {
        let combo = KeyCombo::parse("meta+shift+4").unwrap();
        assert_eq!(combo.modifiers, vec![Modifier::Meta, Modifier::Shift]);
        assert_eq!(combo.key, "4");
    }

    #[test]
    fn test_parse_alt_names() {
        let combo = KeyCombo::parse("cmd+q").unwrap();
        assert_eq!(combo.modifiers, vec![Modifier::Meta]);
        assert_eq!(combo.key, "q");

        let combo = KeyCombo::parse("win+l").unwrap();
        assert_eq!(combo.modifiers, vec![Modifier::Super]);
        assert_eq!(combo.key, "l");
    }

    #[test]
    fn test_build_translation_rules_mac_to_win() {
        let translations = default_translations();
        let rules = build_translation_rules(OsType::MacOS, OsType::Windows, &translations);
        assert!(!rules.is_empty());

        // meta+tab should translate to alt+tab
        let app_switch = rules.iter().find(|r| r.intent == "app_switcher").unwrap();
        assert_eq!(app_switch.from.modifiers, vec![Modifier::Meta]);
        assert_eq!(app_switch.from.key, "tab");
        assert_eq!(app_switch.to.modifiers, vec![Modifier::Alt]);
        assert_eq!(app_switch.to.key, "tab");
    }

    #[test]
    fn test_find_translation() {
        let translations = default_translations();
        let rules = build_translation_rules(OsType::MacOS, OsType::Windows, &translations);

        // simulate pressing meta+tab
        let pressed = KeyCombo {
            modifiers: vec![Modifier::Meta],
            key: "tab".into(),
        };

        let result = find_translation(&pressed, &rules);
        assert!(result.is_some());
        assert_eq!(result.unwrap().intent, "app_switcher");
        assert_eq!(result.unwrap().to.key, "tab");
        assert_eq!(result.unwrap().to.modifiers, vec![Modifier::Alt]);
    }

    #[test]
    fn test_find_translation_no_match() {
        let translations = default_translations();
        let rules = build_translation_rules(OsType::MacOS, OsType::Windows, &translations);

        let pressed = KeyCombo {
            modifiers: vec![Modifier::Meta],
            key: "c".into(),
        };

        // meta+c is not in the translation rules (handled by basic modifier remap)
        assert!(find_translation(&pressed, &rules).is_none());
    }

    #[test]
    fn test_same_os_no_rules() {
        let translations = default_translations();
        let rules = build_translation_rules(OsType::MacOS, OsType::MacOS, &translations);
        assert!(rules.is_empty());
    }

    // --- VK mapping tests ---

    #[test]
    fn test_modifiers_from_vk_state_ctrl_alt() {
        let states = vec![
            (0xA2, true),  // VK_LCONTROL pressed
            (0xA4, true),  // VK_LMENU pressed
            (0xA0, false), // VK_LSHIFT not pressed
            (0x5B, false), // VK_LWIN not pressed
        ];
        let mods = modifiers_from_vk_state(&states);
        assert_eq!(mods, vec![Modifier::Ctrl, Modifier::Alt]);
    }

    #[test]
    fn test_modifiers_from_vk_state_meta() {
        let states = vec![(0x5B, true)]; // VK_LWIN
        let mods = modifiers_from_vk_state(&states);
        assert_eq!(mods, vec![Modifier::Meta]);
    }

    #[test]
    fn test_modifiers_from_vk_state_empty() {
        let states = vec![
            (0xA2, false),
            (0xA4, false),
            (0xA0, false),
            (0x5B, false),
        ];
        let mods = modifiers_from_vk_state(&states);
        assert!(mods.is_empty());
    }

    #[test]
    fn test_key_name_from_vk_tab() {
        assert_eq!(key_name_from_vk(0x09).unwrap(), "tab");
    }

    #[test]
    fn test_key_name_from_vk_letters() {
        assert_eq!(key_name_from_vk(0x41).unwrap(), "a");
        assert_eq!(key_name_from_vk(0x5A).unwrap(), "z");
        assert_eq!(key_name_from_vk(0x51).unwrap(), "q");
    }

    #[test]
    fn test_key_name_from_vk_f_keys() {
        assert_eq!(key_name_from_vk(0x70).unwrap(), "F1");
        assert_eq!(key_name_from_vk(0x73).unwrap(), "F4");
        assert_eq!(key_name_from_vk(0x7B).unwrap(), "F12");
    }

    #[test]
    fn test_combo_from_vk() {
        // meta+tab
        let combo = combo_from_vk(0x09, &[(0x5B, true), (0xA2, false)]).unwrap();
        assert_eq!(combo.key, "tab");
        assert_eq!(combo.modifiers, vec![Modifier::Meta]);
    }

    #[test]
    fn test_vk_roundtrip() {
        // modifier -> vk -> modifier
        assert_eq!(modifier_to_vk(&Modifier::Ctrl), 0xA2);
        assert_eq!(modifier_to_vk(&Modifier::Alt), 0xA4);
        assert_eq!(modifier_to_vk(&Modifier::Shift), 0xA0);
        assert_eq!(modifier_to_vk(&Modifier::Meta), 0x5B);

        // key name -> vk -> key name
        for name in &["tab", "space", "a", "z", "0", "9", "F1", "F12"] {
            let vk = key_name_to_vk(name).unwrap();
            let back = key_name_from_vk(vk).unwrap();
            assert_eq!(back.to_lowercase(), name.to_lowercase(), "roundtrip failed for {name}");
        }
    }

    // --- CG mapping tests ---

    #[test]
    fn test_modifiers_from_cg_flags_cmd() {
        let flags = 0x00100000; // kCGEventFlagMaskCommand
        let mods = modifiers_from_cg_flags(flags);
        assert_eq!(mods, vec![Modifier::Meta]);
    }

    #[test]
    fn test_modifiers_from_cg_flags_combined() {
        let flags = 0x00100000 | 0x00020000; // Command + Shift
        let mods = modifiers_from_cg_flags(flags);
        assert_eq!(mods, vec![Modifier::Meta, Modifier::Shift]);
    }

    #[test]
    fn test_key_name_from_cg_keycode_tab() {
        assert_eq!(key_name_from_cg_keycode(0x30).unwrap(), "tab");
    }

    #[test]
    fn test_key_name_from_cg_keycode_letters() {
        assert_eq!(key_name_from_cg_keycode(0x0C).unwrap(), "q");
        assert_eq!(key_name_from_cg_keycode(0x00).unwrap(), "a");
        assert_eq!(key_name_from_cg_keycode(0x01).unwrap(), "s");
    }

    #[test]
    fn test_combo_from_cg() {
        // Cmd+Q
        let combo = combo_from_cg(0x0C, 0x00100000).unwrap();
        assert_eq!(combo.key, "q");
        assert_eq!(combo.modifiers, vec![Modifier::Meta]);
    }

    #[test]
    fn test_cg_roundtrip() {
        // key name -> cg keycode -> key name
        for name in &["tab", "space", "q", "a", "s", "0", "4", "F1", "F4"] {
            let kc = key_name_to_cg_keycode(name).unwrap();
            let back = key_name_from_cg_keycode(kc).unwrap();
            assert_eq!(back.to_lowercase(), name.to_lowercase(), "CG roundtrip failed for {name}");
        }
    }

    #[test]
    fn test_cg_modifier_flags_roundtrip() {
        assert_eq!(modifier_to_cg_flag(&Modifier::Meta), 0x00100000);
        assert_eq!(modifier_to_cg_flag(&Modifier::Ctrl), 0x00040000);
        assert_eq!(modifier_to_cg_flag(&Modifier::Alt), 0x00080000);
        assert_eq!(modifier_to_cg_flag(&Modifier::Shift), 0x00020000);
    }
}
