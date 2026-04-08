use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Well-known VCP code 0x60 values for monitor input sources.
/// These follow the MCCS specification, but many monitors deviate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InputSource {
    Vga1,
    Vga2,
    Dvi1,
    Dvi2,
    DisplayPort1,
    DisplayPort2,
    Hdmi1,
    Hdmi2,
    /// Raw VCP value for non-standard monitors (USB-C, Thunderbolt, etc.)
    Raw(u16),
}

impl InputSource {
    /// Convert to the VCP 0x60 value, optionally using aliases for resolution.
    pub fn to_vcp_value(&self, _aliases: &HashMap<String, u16>) -> u16 {
        match self {
            Self::Vga1 => 0x01,
            Self::Vga2 => 0x02,
            Self::Dvi1 => 0x03,
            Self::Dvi2 => 0x04,
            Self::DisplayPort1 => 0x0f,
            Self::DisplayPort2 => 0x10,
            Self::Hdmi1 => 0x11,
            Self::Hdmi2 => 0x12,
            Self::Raw(v) => *v,
        }
    }

    /// resolve a raw VCP value to a well-known input source if possible
    pub fn from_vcp_value(vcp: u16) -> Self {
        match vcp {
            0x01 => Self::Vga1,
            0x02 => Self::Vga2,
            0x03 => Self::Dvi1,
            0x04 => Self::Dvi2,
            0x0f => Self::DisplayPort1,
            0x10 => Self::DisplayPort2,
            0x11 => Self::Hdmi1,
            0x12 => Self::Hdmi2,
            v => Self::Raw(v),
        }
    }

    /// Parse from a string, resolving aliases if needed.
    pub fn from_str_with_aliases(s: &str, aliases: &HashMap<String, u16>) -> Option<Self> {
        // Try well-known names first
        let source = match s.to_lowercase().replace(['-', '_', ' '], "").as_str() {
            "vga1" => Some(Self::Vga1),
            "vga2" => Some(Self::Vga2),
            "dvi1" => Some(Self::Dvi1),
            "dvi2" => Some(Self::Dvi2),
            "displayport1" | "dp1" => Some(Self::DisplayPort1),
            "displayport2" | "dp2" => Some(Self::DisplayPort2),
            "hdmi1" => Some(Self::Hdmi1),
            "hdmi2" => Some(Self::Hdmi2),
            _ => None,
        };

        if source.is_some() {
            return source;
        }

        // Try aliases
        if let Some(&raw) = aliases.get(s) {
            return Some(Self::Raw(raw));
        }

        // Try parsing as hex (0x0f) or decimal
        if let Some(hex) = s.strip_prefix("0x") {
            u16::from_str_radix(hex, 16).ok().map(Self::Raw)
        } else {
            s.parse::<u16>().ok().map(Self::Raw)
        }
    }
}

impl fmt::Display for InputSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vga1 => write!(f, "VGA1"),
            Self::Vga2 => write!(f, "VGA2"),
            Self::Dvi1 => write!(f, "DVI1"),
            Self::Dvi2 => write!(f, "DVI2"),
            Self::DisplayPort1 => write!(f, "DisplayPort1"),
            Self::DisplayPort2 => write!(f, "DisplayPort2"),
            Self::Hdmi1 => write!(f, "HDMI1"),
            Self::Hdmi2 => write!(f, "HDMI2"),
            Self::Raw(v) => write!(f, "0x{v:02x}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vcp_values() {
        let aliases = HashMap::new();
        assert_eq!(InputSource::Hdmi1.to_vcp_value(&aliases), 0x11);
        assert_eq!(InputSource::DisplayPort1.to_vcp_value(&aliases), 0x0f);
        assert_eq!(InputSource::Raw(0x1b).to_vcp_value(&aliases), 0x1b);
    }

    #[test]
    fn test_parse_well_known() {
        let aliases = HashMap::new();
        assert_eq!(
            InputSource::from_str_with_aliases("HDMI1", &aliases),
            Some(InputSource::Hdmi1)
        );
        assert_eq!(
            InputSource::from_str_with_aliases("display-port-1", &aliases),
            Some(InputSource::DisplayPort1)
        );
        assert_eq!(
            InputSource::from_str_with_aliases("DP1", &aliases),
            Some(InputSource::DisplayPort1)
        );
    }

    #[test]
    fn test_parse_alias() {
        let mut aliases = HashMap::new();
        aliases.insert("USB-C".to_string(), 0x0f);
        assert_eq!(
            InputSource::from_str_with_aliases("USB-C", &aliases),
            Some(InputSource::Raw(0x0f))
        );
    }

    #[test]
    fn test_parse_hex() {
        let aliases = HashMap::new();
        assert_eq!(
            InputSource::from_str_with_aliases("0x1b", &aliases),
            Some(InputSource::Raw(0x1b))
        );
    }
}
