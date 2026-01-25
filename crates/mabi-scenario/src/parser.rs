//! Scenario parser.

use std::path::Path;

use crate::schema::Scenario;
use crate::{ScenarioError, ScenarioResult};

/// Scenario parser.
pub struct ScenarioParser;

impl ScenarioParser {
    /// Load scenario from YAML file.
    pub async fn load_yaml(path: impl AsRef<Path>) -> ScenarioResult<Scenario> {
        let content = tokio::fs::read_to_string(path).await?;
        let scenario: Scenario = serde_yaml::from_str(&content)?;
        Ok(scenario)
    }

    /// Load scenario from JSON file.
    pub async fn load_json(path: impl AsRef<Path>) -> ScenarioResult<Scenario> {
        let content = tokio::fs::read_to_string(path).await?;
        let scenario: Scenario = serde_json::from_str(&content)?;
        Ok(scenario)
    }

    /// Load scenario from file (auto-detect format).
    pub async fn load(path: impl AsRef<Path>) -> ScenarioResult<Scenario> {
        let path = path.as_ref();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        match ext {
            "yaml" | "yml" => Self::load_yaml(path).await,
            "json" => Self::load_json(path).await,
            _ => Err(ScenarioError::Parse(format!(
                "Unknown file extension: {}",
                ext
            ))),
        }
    }

    /// Parse scenario from YAML string.
    pub fn parse_yaml(content: &str) -> ScenarioResult<Scenario> {
        let scenario: Scenario = serde_yaml::from_str(content)?;
        Ok(scenario)
    }

    /// Parse scenario from JSON string.
    pub fn parse_json(content: &str) -> ScenarioResult<Scenario> {
        let scenario: Scenario = serde_json::from_str(content)?;
        Ok(scenario)
    }

    /// Validate scenario.
    pub fn validate(scenario: &Scenario) -> ScenarioResult<()> {
        if scenario.name.is_empty() {
            return Err(ScenarioError::Parse("Scenario name is required".into()));
        }

        if scenario.time_scale <= 0.0 {
            return Err(ScenarioError::Parse("Time scale must be positive".into()));
        }

        for point in &scenario.points {
            if point.device_id.is_empty() {
                return Err(ScenarioError::Parse(format!(
                    "Point {} has empty device_id",
                    point.id
                )));
            }
        }

        Ok(())
    }
}
