//! Configuration file loading with multiple format support.
//!
//! This module provides a unified configuration loading system that supports:
//! - YAML files (.yaml, .yml)
//! - JSON files (.json)
//! - TOML files (.toml)
//! - Automatic format detection based on file extension
//! - Format-agnostic loading with explicit format specification
//!
//! # Example
//!
//! ```rust,ignore
//! use mabi_core::config::loader::{ConfigLoader, ConfigFormat};
//!
//! // Auto-detect format from extension
//! let config: EngineConfig = ConfigLoader::load("config.yaml")?;
//!
//! // Explicit format
//! let config: EngineConfig = ConfigLoader::load_with_format("config", ConfigFormat::Toml)?;
//!
//! // Load from string
//! let yaml_content = "max_devices: 10000";
//! let config: EngineConfig = ConfigLoader::parse(yaml_content, ConfigFormat::Yaml)?;
//! ```

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::Error;
use crate::Result;

/// Supported configuration file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigFormat {
    /// YAML format (.yaml, .yml)
    Yaml,
    /// JSON format (.json)
    Json,
    /// TOML format (.toml)
    Toml,
}

impl ConfigFormat {
    /// Get format from file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "yaml" | "yml" => Some(Self::Yaml),
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            _ => None,
        }
    }

    /// Get format from file path.
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        path.as_ref()
            .extension()
            .and_then(|e| e.to_str())
            .and_then(Self::from_extension)
    }

    /// Get the primary file extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Yaml => "yaml",
            Self::Json => "json",
            Self::Toml => "toml",
        }
    }

    /// Get all valid extensions for this format.
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Yaml => &["yaml", "yml"],
            Self::Json => &["json"],
            Self::Toml => &["toml"],
        }
    }

    /// Get MIME type for this format.
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Yaml => "application/x-yaml",
            Self::Json => "application/json",
            Self::Toml => "application/toml",
        }
    }
}

impl std::fmt::Display for ConfigFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Yaml => write!(f, "YAML"),
            Self::Json => write!(f, "JSON"),
            Self::Toml => write!(f, "TOML"),
        }
    }
}

/// Configuration loader with format detection and parsing.
pub struct ConfigLoader;

impl ConfigLoader {
    /// Load configuration from a file with automatic format detection.
    ///
    /// Format is determined by file extension:
    /// - `.yaml`, `.yml` -> YAML
    /// - `.json` -> JSON
    /// - `.toml` -> TOML
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - File cannot be read
    /// - Format cannot be determined from extension
    /// - Content fails to parse
    pub fn load<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T> {
        let path = path.as_ref();
        let format = ConfigFormat::from_path(path).ok_or_else(|| {
            Error::Config(format!(
                "Cannot determine format from file extension: {}",
                path.display()
            ))
        })?;

        Self::load_with_format(path, format)
    }

    /// Load configuration from a file with explicit format.
    pub fn load_with_format<T: DeserializeOwned>(
        path: impl AsRef<Path>,
        format: ConfigFormat,
    ) -> Result<T> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)?;
        Self::parse(&content, format)
    }

    /// Load configuration from a reader with explicit format.
    pub fn load_from_reader<T: DeserializeOwned, R: Read>(
        reader: &mut R,
        format: ConfigFormat,
    ) -> Result<T> {
        let mut content = String::new();
        reader.read_to_string(&mut content)?;
        Self::parse(&content, format)
    }

    /// Parse configuration from a string with specified format.
    pub fn parse<T: DeserializeOwned>(content: &str, format: ConfigFormat) -> Result<T> {
        match format {
            ConfigFormat::Yaml => {
                serde_yaml::from_str(content).map_err(|e| Error::Serialization(e.to_string()))
            }
            ConfigFormat::Json => {
                serde_json::from_str(content).map_err(|e| Error::Serialization(e.to_string()))
            }
            ConfigFormat::Toml => {
                toml::from_str(content).map_err(|e| Error::Serialization(e.to_string()))
            }
        }
    }

    /// Serialize configuration to a string.
    pub fn serialize<T: Serialize>(config: &T, format: ConfigFormat) -> Result<String> {
        match format {
            ConfigFormat::Yaml => {
                serde_yaml::to_string(config).map_err(|e| Error::Serialization(e.to_string()))
            }
            ConfigFormat::Json => serde_json::to_string_pretty(config)
                .map_err(|e| Error::Serialization(e.to_string())),
            ConfigFormat::Toml => {
                toml::to_string_pretty(config).map_err(|e| Error::Serialization(e.to_string()))
            }
        }
    }

    /// Save configuration to a file with automatic format detection.
    pub fn save<T: Serialize>(config: &T, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let format = ConfigFormat::from_path(path).ok_or_else(|| {
            Error::Config(format!(
                "Cannot determine format from file extension: {}",
                path.display()
            ))
        })?;

        Self::save_with_format(config, path, format)
    }

    /// Save configuration to a file with explicit format.
    pub fn save_with_format<T: Serialize>(
        config: &T,
        path: impl AsRef<Path>,
        format: ConfigFormat,
    ) -> Result<()> {
        let content = Self::serialize(config, format)?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Try to load configuration from multiple paths, returning the first success.
    ///
    /// Useful for loading from default locations:
    /// ```rust,ignore
    /// let config: EngineConfig = ConfigLoader::load_first(&[
    ///     "config.yaml",
    ///     "config.json",
    ///     "/etc/mabinogion/config.yaml",
    /// ])?;
    /// ```
    pub fn load_first<T: DeserializeOwned>(paths: &[impl AsRef<Path>]) -> Result<(T, PathBuf)> {
        let mut last_error = None;

        for path in paths {
            let path = path.as_ref();
            if path.exists() {
                match Self::load(path) {
                    Ok(config) => return Ok((config, path.to_path_buf())),
                    Err(e) => last_error = Some(e),
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            Error::Config("No configuration file found in any of the specified paths".to_string())
        }))
    }

    /// Check if a file format is supported.
    pub fn is_supported(path: impl AsRef<Path>) -> bool {
        ConfigFormat::from_path(path).is_some()
    }
}

/// Configuration file discovery.
pub struct ConfigDiscovery {
    /// Base name for configuration files (without extension).
    base_name: String,
    /// Search directories in order of precedence.
    search_dirs: Vec<PathBuf>,
    /// Preferred formats in order.
    formats: Vec<ConfigFormat>,
}

impl ConfigDiscovery {
    /// Create a new configuration discovery with default settings.
    pub fn new(base_name: impl Into<String>) -> Self {
        Self {
            base_name: base_name.into(),
            search_dirs: Vec::new(),
            formats: vec![ConfigFormat::Yaml, ConfigFormat::Toml, ConfigFormat::Json],
        }
    }

    /// Add a search directory.
    pub fn search_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.search_dirs.push(dir.into());
        self
    }

    /// Add multiple search directories.
    pub fn search_dirs(mut self, dirs: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        self.search_dirs.extend(dirs.into_iter().map(Into::into));
        self
    }

    /// Set preferred formats.
    pub fn formats(mut self, formats: Vec<ConfigFormat>) -> Self {
        self.formats = formats;
        self
    }

    /// Add standard search locations.
    ///
    /// Includes:
    /// - Current directory
    /// - ./config/
    /// - ~/.config/{app_name}/
    /// - /etc/{app_name}/
    pub fn with_standard_dirs(mut self, app_name: &str) -> Self {
        // Current directory
        self.search_dirs.push(PathBuf::from("."));

        // ./config/
        self.search_dirs.push(PathBuf::from("./config"));

        // User config directory
        if let Some(home) = dirs_home() {
            self.search_dirs.push(home.join(".config").join(app_name));
        }

        // System config directory (Unix)
        #[cfg(unix)]
        {
            self.search_dirs.push(PathBuf::from("/etc").join(app_name));
        }

        self
    }

    /// Find all candidate configuration file paths.
    pub fn candidates(&self) -> Vec<PathBuf> {
        let mut candidates = Vec::new();

        for dir in &self.search_dirs {
            for format in &self.formats {
                for ext in format.extensions() {
                    let path = dir.join(format!("{}.{}", self.base_name, ext));
                    candidates.push(path);
                }
            }
        }

        candidates
    }

    /// Find the first existing configuration file.
    pub fn find(&self) -> Option<PathBuf> {
        self.candidates().into_iter().find(|p| p.exists())
    }

    /// Load configuration from the first found file.
    pub fn load<T: DeserializeOwned>(&self) -> Result<(T, PathBuf)> {
        let candidates = self.candidates();
        ConfigLoader::load_first(&candidates)
    }
}

/// Get home directory (cross-platform).
fn dirs_home() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

/// Builder for creating layered configuration.
///
/// Loads configuration from multiple sources with later sources
/// overriding earlier ones.
pub struct LayeredConfigBuilder<T> {
    base: T,
    loaded_from: Vec<PathBuf>,
}

impl<T: DeserializeOwned + Default + Clone> LayeredConfigBuilder<T> {
    /// Create with default base configuration.
    pub fn new() -> Self {
        Self {
            base: T::default(),
            loaded_from: Vec::new(),
        }
    }

    /// Create with a specific base configuration.
    pub fn with_base(base: T) -> Self {
        Self {
            base,
            loaded_from: Vec::new(),
        }
    }
}

impl<T: DeserializeOwned + Clone> LayeredConfigBuilder<T> {
    /// Load and merge from a file if it exists.
    pub fn load_optional(mut self, path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        if path.exists() {
            if let Ok(config) = ConfigLoader::load::<T>(path) {
                self.base = config;
                self.loaded_from.push(path.to_path_buf());
            }
        }
        self
    }

    /// Load and merge from a file (error if not found).
    pub fn load(mut self, path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        self.base = ConfigLoader::load(path)?;
        self.loaded_from.push(path.to_path_buf());
        Ok(self)
    }

    /// Get the list of files that were loaded.
    pub fn loaded_from(&self) -> &[PathBuf] {
        &self.loaded_from
    }

    /// Build the final configuration.
    pub fn build(self) -> T {
        self.base
    }
}

impl<T: DeserializeOwned + Default + Clone> Default for LayeredConfigBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::io::Cursor;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
    struct TestConfig {
        name: String,
        #[serde(default)]
        count: u32,
    }

    #[test]
    fn test_format_from_extension() {
        assert_eq!(
            ConfigFormat::from_extension("yaml"),
            Some(ConfigFormat::Yaml)
        );
        assert_eq!(
            ConfigFormat::from_extension("yml"),
            Some(ConfigFormat::Yaml)
        );
        assert_eq!(
            ConfigFormat::from_extension("json"),
            Some(ConfigFormat::Json)
        );
        assert_eq!(
            ConfigFormat::from_extension("toml"),
            Some(ConfigFormat::Toml)
        );
        assert_eq!(ConfigFormat::from_extension("txt"), None);
    }

    #[test]
    fn test_format_from_path() {
        assert_eq!(
            ConfigFormat::from_path("config.yaml"),
            Some(ConfigFormat::Yaml)
        );
        assert_eq!(
            ConfigFormat::from_path("/etc/app/config.toml"),
            Some(ConfigFormat::Toml)
        );
        assert_eq!(
            ConfigFormat::from_path("data.json"),
            Some(ConfigFormat::Json)
        );
        assert_eq!(ConfigFormat::from_path("noext"), None);
    }

    #[test]
    fn test_parse_yaml() {
        let yaml = r#"
name: test
count: 42
"#;
        let config: TestConfig = ConfigLoader::parse(yaml, ConfigFormat::Yaml).unwrap();
        assert_eq!(config.name, "test");
        assert_eq!(config.count, 42);
    }

    #[test]
    fn test_parse_json() {
        let json = r#"{"name": "test", "count": 42}"#;
        let config: TestConfig = ConfigLoader::parse(json, ConfigFormat::Json).unwrap();
        assert_eq!(config.name, "test");
        assert_eq!(config.count, 42);
    }

    #[test]
    fn test_parse_toml() {
        let toml = r#"
name = "test"
count = 42
"#;
        let config: TestConfig = ConfigLoader::parse(toml, ConfigFormat::Toml).unwrap();
        assert_eq!(config.name, "test");
        assert_eq!(config.count, 42);
    }

    #[test]
    fn test_serialize_yaml() {
        let config = TestConfig {
            name: "test".to_string(),
            count: 42,
        };
        let yaml = ConfigLoader::serialize(&config, ConfigFormat::Yaml).unwrap();
        assert!(yaml.contains("name: test"));
    }

    #[test]
    fn test_serialize_json() {
        let config = TestConfig {
            name: "test".to_string(),
            count: 42,
        };
        let json = ConfigLoader::serialize(&config, ConfigFormat::Json).unwrap();
        assert!(json.contains("\"name\": \"test\""));
    }

    #[test]
    fn test_serialize_toml() {
        let config = TestConfig {
            name: "test".to_string(),
            count: 42,
        };
        let toml = ConfigLoader::serialize(&config, ConfigFormat::Toml).unwrap();
        assert!(toml.contains("name = \"test\""));
    }

    #[test]
    fn test_load_from_reader() {
        let yaml = "name: reader_test\ncount: 100\n";
        let mut reader = Cursor::new(yaml);
        let config: TestConfig =
            ConfigLoader::load_from_reader(&mut reader, ConfigFormat::Yaml).unwrap();
        assert_eq!(config.name, "reader_test");
        assert_eq!(config.count, 100);
    }

    #[test]
    fn test_is_supported() {
        assert!(ConfigLoader::is_supported("config.yaml"));
        assert!(ConfigLoader::is_supported("config.yml"));
        assert!(ConfigLoader::is_supported("config.json"));
        assert!(ConfigLoader::is_supported("config.toml"));
        assert!(!ConfigLoader::is_supported("config.txt"));
        assert!(!ConfigLoader::is_supported("config"));
    }

    #[test]
    fn test_config_discovery_candidates() {
        let discovery = ConfigDiscovery::new("config")
            .search_dir("/etc/app")
            .search_dir(".");

        let candidates = discovery.candidates();

        // Should have candidates for each dir × format × extension
        assert!(candidates
            .iter()
            .any(|p| p.to_string_lossy().contains("config.yaml")));
        assert!(candidates
            .iter()
            .any(|p| p.to_string_lossy().contains("config.toml")));
        assert!(candidates
            .iter()
            .any(|p| p.to_string_lossy().contains("config.json")));
    }

    #[test]
    fn test_layered_config_builder() {
        let config = LayeredConfigBuilder::<TestConfig>::new().build();
        assert_eq!(config.name, "");
        assert_eq!(config.count, 0);
    }

    #[test]
    fn test_format_display() {
        assert_eq!(format!("{}", ConfigFormat::Yaml), "YAML");
        assert_eq!(format!("{}", ConfigFormat::Json), "JSON");
        assert_eq!(format!("{}", ConfigFormat::Toml), "TOML");
    }

    #[test]
    fn test_format_mime_type() {
        assert_eq!(ConfigFormat::Yaml.mime_type(), "application/x-yaml");
        assert_eq!(ConfigFormat::Json.mime_type(), "application/json");
        assert_eq!(ConfigFormat::Toml.mime_type(), "application/toml");
    }
}
