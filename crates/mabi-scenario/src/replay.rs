//! Replay pattern implementation with file parsing and interpolation.
//!
//! The Replay pattern allows replaying historical data from CSV or JSON files
//! with support for time scaling, looping, and value interpolation.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::ScenarioError;

/// Replay data point with timestamp and value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayDataPoint {
    /// Timestamp in seconds from start.
    pub time_secs: f64,
    /// Value at this timestamp.
    pub value: f64,
}

impl ReplayDataPoint {
    /// Create a new data point.
    pub fn new(time_secs: f64, value: f64) -> Self {
        Self { time_secs, value }
    }
}

/// Replay data series.
#[derive(Debug, Clone)]
pub struct ReplaySeries {
    /// Series name/ID.
    pub name: String,
    /// Data points sorted by time.
    data: Vec<ReplayDataPoint>,
    /// Total duration.
    duration: f64,
}

impl ReplaySeries {
    /// Create a new replay series.
    pub fn new(name: impl Into<String>, mut data: Vec<ReplayDataPoint>) -> Self {
        // Sort by time
        data.sort_by(|a, b| a.time_secs.partial_cmp(&b.time_secs).unwrap());

        let duration = data.last().map(|p| p.time_secs).unwrap_or(0.0);

        Self {
            name: name.into(),
            data,
            duration,
        }
    }

    /// Get value at specific time with interpolation.
    pub fn get_value_at(&self, time_secs: f64) -> Option<f64> {
        if self.data.is_empty() {
            return None;
        }

        // Handle edge cases
        if time_secs <= self.data[0].time_secs {
            return Some(self.data[0].value);
        }
        if time_secs >= self.duration {
            return Some(self.data.last()?.value);
        }

        // Binary search for interpolation
        let idx = self
            .data
            .binary_search_by(|p| p.time_secs.partial_cmp(&time_secs).unwrap())
            .unwrap_or_else(|i| i.saturating_sub(1));

        if idx + 1 >= self.data.len() {
            return Some(self.data[idx].value);
        }

        // Linear interpolation
        let p1 = &self.data[idx];
        let p2 = &self.data[idx + 1];

        let t = (time_secs - p1.time_secs) / (p2.time_secs - p1.time_secs);
        Some(p1.value + (p2.value - p1.value) * t)
    }

    /// Get value with looping.
    pub fn get_value_looped(&self, time_secs: f64) -> Option<f64> {
        if self.duration <= 0.0 {
            return self.data.first().map(|p| p.value);
        }

        let looped_time = time_secs % self.duration;
        self.get_value_at(looped_time)
    }

    /// Get total duration.
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// Get number of data points.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if series is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get all data points.
    pub fn data(&self) -> &[ReplayDataPoint] {
        &self.data
    }
}

/// Interpolation method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterpolationMethod {
    /// No interpolation, use nearest value.
    Nearest,
    /// Linear interpolation between points.
    #[default]
    Linear,
    /// Step function (hold previous value).
    Step,
}

/// Replay configuration.
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// Source file path.
    pub file: PathBuf,
    /// Whether to loop the replay.
    pub loop_replay: bool,
    /// Interpolation method.
    pub interpolation: InterpolationMethod,
    /// Time column name (for CSV).
    pub time_column: String,
    /// Value column name (for CSV).
    pub value_column: String,
    /// Time scale factor.
    pub time_scale: f64,
}

impl ReplayConfig {
    /// Create a new replay configuration.
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            loop_replay: false,
            interpolation: InterpolationMethod::Linear,
            time_column: "time".to_string(),
            value_column: "value".to_string(),
            time_scale: 1.0,
        }
    }

    /// Set loop replay.
    pub fn with_loop(mut self, loop_replay: bool) -> Self {
        self.loop_replay = loop_replay;
        self
    }

    /// Set interpolation method.
    pub fn with_interpolation(mut self, method: InterpolationMethod) -> Self {
        self.interpolation = method;
        self
    }

    /// Set time column name.
    pub fn with_time_column(mut self, column: impl Into<String>) -> Self {
        self.time_column = column.into();
        self
    }

    /// Set value column name.
    pub fn with_value_column(mut self, column: impl Into<String>) -> Self {
        self.value_column = column.into();
        self
    }

    /// Set time scale.
    pub fn with_time_scale(mut self, scale: f64) -> Self {
        self.time_scale = scale;
        self
    }
}

/// Replay file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayFormat {
    /// CSV format.
    Csv,
    /// JSON format.
    Json,
    /// JSON Lines format.
    JsonLines,
}

impl ReplayFormat {
    /// Detect format from file extension.
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("csv") => Some(Self::Csv),
            Some("json") => Some(Self::Json),
            Some("jsonl") | Some("ndjson") => Some(Self::JsonLines),
            _ => None,
        }
    }
}

/// Replay data loader.
pub struct ReplayLoader {
    /// Base directory for relative paths.
    base_dir: Option<PathBuf>,
}

impl ReplayLoader {
    /// Create a new replay loader.
    pub fn new() -> Self {
        Self { base_dir: None }
    }

    /// Set base directory.
    pub fn with_base_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.base_dir = Some(dir.into());
        self
    }

    /// Resolve file path.
    fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else if let Some(ref base) = self.base_dir {
            base.join(path)
        } else {
            path.to_path_buf()
        }
    }

    /// Load replay data from file.
    pub fn load(&self, config: &ReplayConfig) -> Result<ReplaySeries, ScenarioError> {
        let path = self.resolve_path(&config.file);
        let format = ReplayFormat::from_path(&path)
            .ok_or_else(|| ScenarioError::Parse(format!("Unknown file format: {:?}", path)))?;

        match format {
            ReplayFormat::Csv => self.load_csv(&path, config),
            ReplayFormat::Json => self.load_json(&path, config),
            ReplayFormat::JsonLines => self.load_jsonl(&path, config),
        }
    }

    /// Load from CSV file.
    fn load_csv(&self, path: &Path, config: &ReplayConfig) -> Result<ReplaySeries, ScenarioError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();

        // Parse header
        let header_line = lines
            .next()
            .ok_or_else(|| ScenarioError::Parse("Empty CSV file".to_string()))??;

        let headers: Vec<&str> = header_line.split(',').map(|s| s.trim()).collect();

        let time_idx = headers
            .iter()
            .position(|&h| h == config.time_column)
            .ok_or_else(|| {
                ScenarioError::Parse(format!("Time column '{}' not found", config.time_column))
            })?;

        let value_idx = headers
            .iter()
            .position(|&h| h == config.value_column)
            .ok_or_else(|| {
                ScenarioError::Parse(format!("Value column '{}' not found", config.value_column))
            })?;

        // Parse data rows
        let mut data = Vec::new();
        for (line_num, line_result) in lines.enumerate() {
            let line = line_result?;
            let fields: Vec<&str> = line.split(',').map(|s| s.trim()).collect();

            if fields.len() <= time_idx.max(value_idx) {
                continue; // Skip malformed rows
            }

            let time_secs = fields[time_idx].parse::<f64>().map_err(|e| {
                ScenarioError::Parse(format!("Invalid time at line {}: {}", line_num + 2, e))
            })? * config.time_scale;

            let value = fields[value_idx].parse::<f64>().map_err(|e| {
                ScenarioError::Parse(format!("Invalid value at line {}: {}", line_num + 2, e))
            })?;

            data.push(ReplayDataPoint::new(time_secs, value));
        }

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("replay");
        Ok(ReplaySeries::new(name, data))
    }

    /// Load from JSON file.
    fn load_json(&self, path: &Path, config: &ReplayConfig) -> Result<ReplaySeries, ScenarioError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        #[derive(Deserialize)]
        struct JsonData {
            data: Vec<HashMap<String, serde_json::Value>>,
        }

        let json: JsonData = serde_json::from_reader(reader)?;

        let mut data = Vec::new();
        for entry in json.data {
            let time_secs = entry
                .get(&config.time_column)
                .and_then(|v| v.as_f64())
                .ok_or_else(|| {
                    ScenarioError::Parse(format!("Missing time column '{}'", config.time_column))
                })?
                * config.time_scale;

            let value = entry
                .get(&config.value_column)
                .and_then(|v| v.as_f64())
                .ok_or_else(|| {
                    ScenarioError::Parse(format!("Missing value column '{}'", config.value_column))
                })?;

            data.push(ReplayDataPoint::new(time_secs, value));
        }

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("replay");
        Ok(ReplaySeries::new(name, data))
    }

    /// Load from JSON Lines file.
    fn load_jsonl(
        &self,
        path: &Path,
        config: &ReplayConfig,
    ) -> Result<ReplaySeries, ScenarioError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut data = Vec::new();
        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result?;
            if line.trim().is_empty() {
                continue;
            }

            let entry: HashMap<String, serde_json::Value> =
                serde_json::from_str(&line).map_err(|e| {
                    ScenarioError::Parse(format!("Invalid JSON at line {}: {}", line_num + 1, e))
                })?;

            let time_secs = entry
                .get(&config.time_column)
                .and_then(|v| v.as_f64())
                .ok_or_else(|| {
                    ScenarioError::Parse(format!(
                        "Missing time column '{}' at line {}",
                        config.time_column,
                        line_num + 1
                    ))
                })?
                * config.time_scale;

            let value = entry
                .get(&config.value_column)
                .and_then(|v| v.as_f64())
                .ok_or_else(|| {
                    ScenarioError::Parse(format!(
                        "Missing value column '{}' at line {}",
                        config.value_column,
                        line_num + 1
                    ))
                })?;

            data.push(ReplayDataPoint::new(time_secs, value));
        }

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("replay");
        Ok(ReplaySeries::new(name, data))
    }
}

impl Default for ReplayLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Replay state for playback.
pub struct ReplayState {
    /// Replay series data.
    series: ReplaySeries,
    /// Configuration.
    config: ReplayConfig,
    /// Current playback time.
    current_time: f64,
    /// Last generated value.
    last_value: f64,
    /// Whether playback is complete.
    completed: bool,
}

impl ReplayState {
    /// Create a new replay state.
    pub fn new(series: ReplaySeries, config: ReplayConfig) -> Self {
        let last_value = series.get_value_at(0.0).unwrap_or(0.0);
        Self {
            series,
            config,
            current_time: 0.0,
            last_value,
            completed: false,
        }
    }

    /// Generate value at specific time.
    pub fn generate_at(&mut self, time_secs: f64) -> f64 {
        let scaled_time = time_secs * self.config.time_scale;

        if self.config.loop_replay {
            self.last_value = self.interpolate(self.series.get_value_looped(scaled_time));
        } else if scaled_time >= self.series.duration() {
            self.completed = true;
            self.last_value = self
                .series
                .get_value_at(self.series.duration())
                .unwrap_or(self.last_value);
        } else {
            self.last_value = self.interpolate(self.series.get_value_at(scaled_time));
        }

        self.current_time = scaled_time;
        self.last_value
    }

    /// Apply interpolation method.
    fn interpolate(&self, value: Option<f64>) -> f64 {
        match self.config.interpolation {
            InterpolationMethod::Linear | InterpolationMethod::Nearest => {
                value.unwrap_or(self.last_value)
            }
            InterpolationMethod::Step => value.unwrap_or(self.last_value),
        }
    }

    /// Get current time.
    pub fn current_time(&self) -> f64 {
        self.current_time
    }

    /// Get last value.
    pub fn last_value(&self) -> f64 {
        self.last_value
    }

    /// Check if playback is complete.
    pub fn is_completed(&self) -> bool {
        self.completed && !self.config.loop_replay
    }

    /// Reset playback.
    pub fn reset(&mut self) {
        self.current_time = 0.0;
        self.last_value = self.series.get_value_at(0.0).unwrap_or(0.0);
        self.completed = false;
    }

    /// Get series duration.
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.series.duration() / self.config.time_scale)
    }
}

/// Replay manager for multiple replay series.
#[derive(Default)]
pub struct ReplayManager {
    /// Loader.
    loader: ReplayLoader,
    /// Active replay states by point ID.
    states: HashMap<String, ReplayState>,
}

impl ReplayManager {
    /// Create a new replay manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set base directory.
    pub fn with_base_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.loader = self.loader.with_base_dir(dir);
        self
    }

    /// Register a replay for a point.
    pub fn register(
        &mut self,
        point_id: impl Into<String>,
        config: ReplayConfig,
    ) -> Result<(), ScenarioError> {
        let series = self.loader.load(&config)?;
        let state = ReplayState::new(series, config);
        self.states.insert(point_id.into(), state);
        Ok(())
    }

    /// Generate value for a point at specific time.
    pub fn generate_at(&mut self, point_id: &str, time_secs: f64) -> Option<f64> {
        self.states
            .get_mut(point_id)
            .map(|s| s.generate_at(time_secs))
    }

    /// Get all point values at specific time.
    pub fn generate_all_at(&mut self, time_secs: f64) -> Vec<(String, f64)> {
        self.states
            .iter_mut()
            .map(|(id, state)| (id.clone(), state.generate_at(time_secs)))
            .collect()
    }

    /// Reset all replays.
    pub fn reset(&mut self) {
        for state in self.states.values_mut() {
            state.reset();
        }
    }

    /// Check if all replays are complete.
    pub fn all_completed(&self) -> bool {
        self.states.values().all(|s| s.is_completed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_series() -> ReplaySeries {
        let data = vec![
            ReplayDataPoint::new(0.0, 0.0),
            ReplayDataPoint::new(1.0, 10.0),
            ReplayDataPoint::new(2.0, 20.0),
            ReplayDataPoint::new(3.0, 30.0),
        ];
        ReplaySeries::new("test", data)
    }

    #[test]
    fn test_replay_series_interpolation() {
        let series = create_test_series();

        // Exact points
        assert!((series.get_value_at(0.0).unwrap() - 0.0).abs() < 0.001);
        assert!((series.get_value_at(1.0).unwrap() - 10.0).abs() < 0.001);
        assert!((series.get_value_at(3.0).unwrap() - 30.0).abs() < 0.001);

        // Interpolated
        assert!((series.get_value_at(0.5).unwrap() - 5.0).abs() < 0.001);
        assert!((series.get_value_at(1.5).unwrap() - 15.0).abs() < 0.001);

        // Edge cases
        assert!((series.get_value_at(-1.0).unwrap() - 0.0).abs() < 0.001);
        assert!((series.get_value_at(5.0).unwrap() - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_replay_series_looping() {
        let series = create_test_series();

        // First loop
        assert!((series.get_value_looped(0.0).unwrap() - 0.0).abs() < 0.001);
        assert!((series.get_value_looped(1.5).unwrap() - 15.0).abs() < 0.001);

        // Second loop
        assert!((series.get_value_looped(3.5).unwrap() - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_csv_loading() {
        let mut file = NamedTempFile::with_suffix(".csv").unwrap();
        writeln!(file, "time,value").unwrap();
        writeln!(file, "0.0,10.0").unwrap();
        writeln!(file, "1.0,20.0").unwrap();
        writeln!(file, "2.0,30.0").unwrap();

        let config = ReplayConfig::new(file.path());
        let loader = ReplayLoader::new();
        let series = loader.load(&config).unwrap();

        assert_eq!(series.len(), 3);
        assert!((series.get_value_at(0.5).unwrap() - 15.0).abs() < 0.001);
    }

    #[test]
    fn test_jsonl_loading() {
        let mut file = NamedTempFile::with_suffix(".jsonl").unwrap();
        writeln!(file, r#"{{"time": 0.0, "value": 10.0}}"#).unwrap();
        writeln!(file, r#"{{"time": 1.0, "value": 20.0}}"#).unwrap();
        writeln!(file, r#"{{"time": 2.0, "value": 30.0}}"#).unwrap();

        let config = ReplayConfig::new(file.path());
        let loader = ReplayLoader::new();
        let series = loader.load(&config).unwrap();

        assert_eq!(series.len(), 3);
        assert!((series.get_value_at(1.0).unwrap() - 20.0).abs() < 0.001);
    }

    #[test]
    fn test_replay_state() {
        let series = create_test_series();
        let config = ReplayConfig::new("test.csv");
        let mut state = ReplayState::new(series, config);

        assert!((state.generate_at(0.0) - 0.0).abs() < 0.001);
        assert!((state.generate_at(1.5) - 15.0).abs() < 0.001);
        assert!(!state.is_completed());

        // Go past end
        let _ = state.generate_at(5.0);
        assert!(state.is_completed());
    }

    #[test]
    fn test_replay_config_builder() {
        let config = ReplayConfig::new("data.csv")
            .with_loop(true)
            .with_interpolation(InterpolationMethod::Step)
            .with_time_column("timestamp")
            .with_value_column("reading")
            .with_time_scale(2.0);

        assert!(config.loop_replay);
        assert_eq!(config.interpolation, InterpolationMethod::Step);
        assert_eq!(config.time_column, "timestamp");
        assert_eq!(config.value_column, "reading");
        assert!((config.time_scale - 2.0).abs() < f64::EPSILON);
    }
}
