use regex::Regex;
use std::sync::LazyLock;

/// a parsed screen transition event from deskflow's log output
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionEvent {
    pub source: String,
    pub target: String,
    pub x: i32,
    pub y: i32,
}

static TRANSITION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"switch from "(.+?)" to "(.+?)" at (\d+),(\d+)"#)
        .expect("transition regex is valid")
});

/// try to parse a deskflow log line into a transition event
pub fn parse_line(line: &str) -> Option<TransitionEvent> {
    let caps = TRANSITION_RE.captures(line)?;
    Some(TransitionEvent {
        source: caps[1].to_string(),
        target: caps[2].to_string(),
        x: caps[3].parse().ok()?,
        y: caps[4].parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_transition() {
        let line = r#"[2024-03-15T10:30:00] INFO: switch from "Windows-PC" to "MacBook" at 1920,540"#;
        let event = parse_line(line).unwrap();
        assert_eq!(event.source, "Windows-PC");
        assert_eq!(event.target, "MacBook");
        assert_eq!(event.x, 1920);
        assert_eq!(event.y, 540);
    }

    #[test]
    fn test_parse_non_transition_line() {
        assert!(parse_line("INFO: connected to client").is_none());
        assert!(parse_line("").is_none());
    }

    #[test]
    fn test_parse_names_with_special_chars() {
        let line = r#"switch from "My-PC_2" to "Mac Book Pro" at 0,100"#;
        let event = parse_line(line).unwrap();
        assert_eq!(event.source, "My-PC_2");
        assert_eq!(event.target, "Mac Book Pro");
    }
}
