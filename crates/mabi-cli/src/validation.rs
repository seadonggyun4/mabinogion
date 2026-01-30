//! Reusable CLI argument validators.
//!
//! Provides `value_parser` compatible functions for clap argument validation.
//! Each validator returns `Result<T, String>` as required by clap.

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
}
