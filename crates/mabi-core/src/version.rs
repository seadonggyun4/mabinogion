//! Workspace release version utilities.
//!
//! Mabinogion manages the human-edited release version from the workspace root.
//! Downstream crates should use this module for externally visible product
//! version strings instead of introducing their own literals.

/// Canonical Mabinogion release version.
pub const RELEASE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Return the canonical Mabinogion release version.
pub const fn release_version() -> &'static str {
    RELEASE_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_version_matches_package_version() {
        assert_eq!(RELEASE_VERSION, env!("CARGO_PKG_VERSION"));
    }
}
