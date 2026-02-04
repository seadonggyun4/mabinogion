//! Reusable CLI argument validators.
//!
//! Provides `value_parser` compatible functions for clap argument validation.
//! Each validator returns `Result<T, String>` as required by clap.

use mabi_core::tags::{parse_tag_string, Tags};

/// Validates that a port number is within the usable range (1–65535).
///
/// Port 0 is rejected because it causes OS-assigned ephemeral port binding,
/// which is not meaningful for a simulator that clients need to connect to.
pub fn parse_port(s: &str) -> Result<u16, String> {
    let port: u16 = s
        .parse()
        .map_err(|_| format!("'{s}' is not a valid port number"))?;
    if port == 0 {
        return Err("port must be between 1 and 65535 (port 0 is not allowed)".to_string());
    }
    Ok(port)
}

/// Validates that a count value is at least 1.
///
/// Zero-count resources (devices, objects, nodes, groups) produce a server
/// with nothing to simulate, which is almost certainly a user mistake.
pub fn parse_nonzero_count(s: &str) -> Result<usize, String> {
    let n: usize = s
        .parse()
        .map_err(|_| format!("'{s}' is not a valid number"))?;
    if n == 0 {
        return Err("value must be at least 1".to_string());
    }
    Ok(n)
}

/// Tag entry parsed from CLI argument.
///
/// Represents either a key-value tag or a label (key only).
#[derive(Debug, Clone)]
pub struct TagEntry {
    pub key: String,
    pub value: Option<String>,
}

/// Parses a tag argument in the format "key=value" or "label".
///
/// - "location=building-a" -> TagEntry { key: "location", value: Some("building-a") }
/// - "critical" -> TagEntry { key: "critical", value: None } (label)
pub fn parse_tag(s: &str) -> Result<TagEntry, String> {
    let (key, value) = parse_tag_string(s)?;
    Ok(TagEntry { key, value })
}

/// Converts a slice of TagEntry into a Tags object.
pub fn tags_from_entries(entries: &[TagEntry]) -> Tags {
    let mut tags = Tags::new();
    for entry in entries {
        match &entry.value {
            Some(v) => tags.insert(&entry.key, v),
            None => tags.add_label(&entry.key),
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_port_valid() {
        assert_eq!(parse_port("1").unwrap(), 1);
        assert_eq!(parse_port("3671").unwrap(), 3671);
        assert_eq!(parse_port("65535").unwrap(), 65535);
    }

    #[test]
    fn test_parse_port_zero_rejected() {
        assert!(parse_port("0").is_err());
    }

    #[test]
    fn test_parse_port_invalid_string() {
        assert!(parse_port("abc").is_err());
        assert!(parse_port("-1").is_err());
        assert!(parse_port("99999").is_err());
    }

    #[test]
    fn test_parse_nonzero_count_valid() {
        assert_eq!(parse_nonzero_count("1").unwrap(), 1);
        assert_eq!(parse_nonzero_count("50000").unwrap(), 50000);
    }

    #[test]
    fn test_parse_nonzero_count_zero_rejected() {
        assert!(parse_nonzero_count("0").is_err());
    }

    #[test]
    fn test_parse_nonzero_count_invalid() {
        assert!(parse_nonzero_count("abc").is_err());
        assert!(parse_nonzero_count("-1").is_err());
    }

    #[test]
    fn test_parse_tag_key_value() {
        let entry = parse_tag("location=building-a").unwrap();
        assert_eq!(entry.key, "location");
        assert_eq!(entry.value, Some("building-a".to_string()));
    }

    #[test]
    fn test_parse_tag_label() {
        let entry = parse_tag("critical").unwrap();
        assert_eq!(entry.key, "critical");
        assert_eq!(entry.value, None);
    }

    #[test]
    fn test_parse_tag_empty_value() {
        let entry = parse_tag("key=").unwrap();
        assert_eq!(entry.key, "key");
        assert_eq!(entry.value, Some("".to_string()));
    }

    #[test]
    fn test_parse_tag_invalid() {
        assert!(parse_tag("").is_err());
        assert!(parse_tag("=value").is_err());
    }

    #[test]
    fn test_tags_from_entries() {
        let entries = vec![
            TagEntry { key: "location".to_string(), value: Some("building-a".to_string()) },
            TagEntry { key: "floor".to_string(), value: Some("3".to_string()) },
            TagEntry { key: "critical".to_string(), value: None },
        ];

        let tags = tags_from_entries(&entries);
        assert_eq!(tags.get("location"), Some("building-a"));
        assert_eq!(tags.get("floor"), Some("3"));
        assert!(tags.has_label("critical"));
    }
}
