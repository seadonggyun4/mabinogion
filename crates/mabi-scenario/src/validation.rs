//! Comprehensive schema validation for scenarios.
//!
//! Provides validation for:
//! - Scenario structure
//! - Point configurations
//! - Pattern parameters
//! - Event definitions
//! - Cross-references (follow sources, event targets)

use std::collections::{HashMap, HashSet};
use std::path::Path;

use mabi_core::tags::Tags;

use crate::schema::{EventAction, EventTrigger, PatternConfig, Scenario};

/// Validation severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValidationSeverity {
    /// Informational message.
    Info,
    /// Warning - scenario will work but might not behave as expected.
    Warning,
    /// Error - scenario will not work correctly.
    Error,
}

/// Validation issue.
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// Issue severity.
    pub severity: ValidationSeverity,
    /// Issue code.
    pub code: ValidationCode,
    /// Human-readable message.
    pub message: String,
    /// Path to the problematic element (e.g., "points[0].pattern").
    pub path: String,
}

impl ValidationIssue {
    /// Create a new validation issue.
    pub fn new(
        severity: ValidationSeverity,
        code: ValidationCode,
        message: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            path: path.into(),
        }
    }

    /// Create an error.
    pub fn error(code: ValidationCode, message: impl Into<String>, path: impl Into<String>) -> Self {
        Self::new(ValidationSeverity::Error, code, message, path)
    }

    /// Create a warning.
    pub fn warning(code: ValidationCode, message: impl Into<String>, path: impl Into<String>) -> Self {
        Self::new(ValidationSeverity::Warning, code, message, path)
    }

    /// Create an info.
    pub fn info(code: ValidationCode, message: impl Into<String>, path: impl Into<String>) -> Self {
        Self::new(ValidationSeverity::Info, code, message, path)
    }
}

/// Validation issue codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValidationCode {
    // Scenario level
    EmptyName,
    InvalidDuration,
    InvalidTimeScale,
    NoPoints,

    // Point level
    DuplicatePointId,
    EmptyPointId,
    EmptyDeviceId,
    InvalidInterval,

    // Pattern level
    InvalidAmplitude,
    InvalidPeriod,
    InvalidRampDuration,
    EmptyStepLevels,
    InvalidRandomRange,
    InvalidNoiseStdDev,
    FollowSourceNotFound,
    FollowCircularDependency,
    ReplayFileNotFound,
    ReplayInvalidFormat,

    // Event level
    DuplicateEventName,
    EmptyEventName,
    InvalidTriggerTime,
    InvalidTriggerInterval,
    InvalidConditionOperator,
    EventTargetNotFound,
    NoEventActions,

    // General
    Deprecated,
    PerformanceWarning,
}

/// Validation result.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// All issues found.
    issues: Vec<ValidationIssue>,
}

impl ValidationResult {
    /// Create empty result.
    pub fn new() -> Self {
        Self { issues: Vec::new() }
    }

    /// Add an issue.
    pub fn add(&mut self, issue: ValidationIssue) {
        self.issues.push(issue);
    }

    /// Add multiple issues.
    pub fn extend(&mut self, issues: impl IntoIterator<Item = ValidationIssue>) {
        self.issues.extend(issues);
    }

    /// Get all issues.
    pub fn issues(&self) -> &[ValidationIssue] {
        &self.issues
    }

    /// Get errors only.
    pub fn errors(&self) -> Vec<&ValidationIssue> {
        self.issues
            .iter()
            .filter(|i| i.severity == ValidationSeverity::Error)
            .collect()
    }

    /// Get warnings.
    pub fn warnings(&self) -> Vec<&ValidationIssue> {
        self.issues
            .iter()
            .filter(|i| i.severity == ValidationSeverity::Warning)
            .collect()
    }

    /// Check if validation passed (no errors).
    pub fn is_valid(&self) -> bool {
        !self.issues.iter().any(|i| i.severity == ValidationSeverity::Error)
    }

    /// Check if there are any issues.
    pub fn has_issues(&self) -> bool {
        !self.issues.is_empty()
    }

    /// Get error count.
    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == ValidationSeverity::Error)
            .count()
    }

    /// Get warning count.
    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == ValidationSeverity::Warning)
            .count()
    }
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Validation options.
#[derive(Debug, Clone)]
pub struct ValidationOptions {
    /// Check if follow sources exist.
    pub check_follow_sources: bool,
    /// Check if replay files exist.
    pub check_replay_files: bool,
    /// Check for circular dependencies.
    pub check_circular_deps: bool,
    /// Warn about performance issues.
    pub warn_performance: bool,
    /// Base directory for file checks.
    pub base_dir: Option<String>,
    /// Maximum recommended points.
    pub max_recommended_points: usize,
    /// Maximum recommended events.
    pub max_recommended_events: usize,
}

impl Default for ValidationOptions {
    fn default() -> Self {
        Self {
            check_follow_sources: true,
            check_replay_files: false, // Disabled by default (might be slow)
            check_circular_deps: true,
            warn_performance: true,
            base_dir: None,
            max_recommended_points: 10_000,
            max_recommended_events: 1_000,
        }
    }
}

/// Scenario validator.
pub struct ScenarioValidator {
    options: ValidationOptions,
}

impl ScenarioValidator {
    /// Create a new validator with default options.
    pub fn new() -> Self {
        Self {
            options: ValidationOptions::default(),
        }
    }

    /// Create with custom options.
    pub fn with_options(options: ValidationOptions) -> Self {
        Self { options }
    }

    /// Validate a scenario.
    pub fn validate(&self, scenario: &Scenario) -> ValidationResult {
        let mut result = ValidationResult::new();

        // Validate scenario level
        self.validate_scenario(&mut result, scenario);

        // Validate points
        self.validate_points(&mut result, scenario);

        // Validate events
        self.validate_events(&mut result, scenario);

        // Cross-reference validation
        if self.options.check_follow_sources || self.options.check_circular_deps {
            self.validate_references(&mut result, scenario);
        }

        // Performance warnings
        if self.options.warn_performance {
            self.validate_performance(&mut result, scenario);
        }

        result
    }

    /// Validate scenario-level properties.
    fn validate_scenario(&self, result: &mut ValidationResult, scenario: &Scenario) {
        // Name validation
        if scenario.name.is_empty() {
            result.add(ValidationIssue::error(
                ValidationCode::EmptyName,
                "Scenario name cannot be empty",
                "name",
            ));
        }

        // Time scale validation
        if scenario.time_scale <= 0.0 {
            result.add(ValidationIssue::error(
                ValidationCode::InvalidTimeScale,
                format!("Time scale must be positive, got {}", scenario.time_scale),
                "time_scale",
            ));
        } else if scenario.time_scale > 1000.0 {
            result.add(ValidationIssue::warning(
                ValidationCode::PerformanceWarning,
                format!("Very high time scale ({}) may cause timing issues", scenario.time_scale),
                "time_scale",
            ));
        }

        // Points validation
        if scenario.points.is_empty() {
            result.add(ValidationIssue::warning(
                ValidationCode::NoPoints,
                "Scenario has no data points defined",
                "points",
            ));
        }
    }

    /// Validate points.
    fn validate_points(&self, result: &mut ValidationResult, scenario: &Scenario) {
        let mut point_ids = HashSet::new();

        for (idx, point) in scenario.points.iter().enumerate() {
            let path = format!("points[{}]", idx);

            // Unique ID check
            if point_ids.contains(&point.id) {
                result.add(ValidationIssue::error(
                    ValidationCode::DuplicatePointId,
                    format!("Duplicate point ID: '{}'", point.id),
                    &path,
                ));
            } else {
                point_ids.insert(point.id.clone());
            }

            // Empty ID check
            if point.id.is_empty() {
                result.add(ValidationIssue::error(
                    ValidationCode::EmptyPointId,
                    "Point ID cannot be empty",
                    format!("{}.id", path),
                ));
            }

            // Device ID check
            if point.device_id.is_empty() {
                result.add(ValidationIssue::error(
                    ValidationCode::EmptyDeviceId,
                    "Device ID cannot be empty",
                    format!("{}.device_id", path),
                ));
            }

            // Interval check
            if point.interval_ms == 0 {
                result.add(ValidationIssue::warning(
                    ValidationCode::InvalidInterval,
                    "Interval of 0ms will cause very high CPU usage",
                    format!("{}.interval_ms", path),
                ));
            }

            // Pattern validation
            self.validate_pattern(result, &point.pattern, format!("{}.pattern", path));
        }
    }

    /// Validate a pattern configuration.
    fn validate_pattern(&self, result: &mut ValidationResult, pattern: &PatternConfig, path: String) {
        match pattern {
            PatternConfig::Sine { amplitude, period_secs, .. }
            | PatternConfig::Cosine { amplitude, period_secs, .. } => {
                if *amplitude < 0.0 {
                    result.add(ValidationIssue::warning(
                        ValidationCode::InvalidAmplitude,
                        "Negative amplitude will invert the wave",
                        &path,
                    ));
                }
                if *period_secs <= 0.0 {
                    result.add(ValidationIssue::error(
                        ValidationCode::InvalidPeriod,
                        format!("Period must be positive, got {}", period_secs),
                        &path,
                    ));
                }
            }

            PatternConfig::Ramp { duration_secs, .. } => {
                if *duration_secs <= 0.0 {
                    result.add(ValidationIssue::error(
                        ValidationCode::InvalidRampDuration,
                        format!("Ramp duration must be positive, got {}", duration_secs),
                        &path,
                    ));
                }
            }

            PatternConfig::Step { levels, step_duration_secs } => {
                if levels.is_empty() {
                    result.add(ValidationIssue::error(
                        ValidationCode::EmptyStepLevels,
                        "Step pattern must have at least one level",
                        &path,
                    ));
                }
                if *step_duration_secs <= 0.0 {
                    result.add(ValidationIssue::error(
                        ValidationCode::InvalidRampDuration,
                        format!("Step duration must be positive, got {}", step_duration_secs),
                        &path,
                    ));
                }
            }

            PatternConfig::Random { min, max, .. } => {
                if min >= max {
                    result.add(ValidationIssue::error(
                        ValidationCode::InvalidRandomRange,
                        format!("Random min ({}) must be less than max ({})", min, max),
                        &path,
                    ));
                }
            }

            PatternConfig::Noise { std_dev, .. } => {
                if *std_dev <= 0.0 {
                    result.add(ValidationIssue::error(
                        ValidationCode::InvalidNoiseStdDev,
                        format!("Noise std_dev must be positive, got {}", std_dev),
                        &path,
                    ));
                }
            }

            PatternConfig::Follow { source, .. } => {
                if source.is_empty() {
                    result.add(ValidationIssue::error(
                        ValidationCode::FollowSourceNotFound,
                        "Follow source cannot be empty",
                        &path,
                    ));
                }
            }

            PatternConfig::Replay { file, .. } => {
                if file.is_empty() {
                    result.add(ValidationIssue::error(
                        ValidationCode::ReplayFileNotFound,
                        "Replay file path cannot be empty",
                        &path,
                    ));
                } else if self.options.check_replay_files {
                    let file_path = if let Some(ref base) = self.options.base_dir {
                        Path::new(base).join(file)
                    } else {
                        Path::new(file).to_path_buf()
                    };

                    if !file_path.exists() {
                        result.add(ValidationIssue::error(
                            ValidationCode::ReplayFileNotFound,
                            format!("Replay file not found: {:?}", file_path),
                            &path,
                        ));
                    }
                }
            }

            PatternConfig::Constant { .. } => {
                // No validation needed
            }
        }
    }

    /// Validate events.
    fn validate_events(&self, result: &mut ValidationResult, scenario: &Scenario) {
        let mut event_names = HashSet::new();

        for (idx, event) in scenario.events.iter().enumerate() {
            let path = format!("events[{}]", idx);

            // Unique name check
            if event_names.contains(&event.name) {
                result.add(ValidationIssue::warning(
                    ValidationCode::DuplicateEventName,
                    format!("Duplicate event name: '{}'", event.name),
                    &path,
                ));
            } else {
                event_names.insert(event.name.clone());
            }

            // Empty name check
            if event.name.is_empty() {
                result.add(ValidationIssue::warning(
                    ValidationCode::EmptyEventName,
                    "Event name is empty",
                    format!("{}.name", path),
                ));
            }

            // Trigger validation
            self.validate_trigger(result, &event.trigger, format!("{}.trigger", path));

            // Actions validation
            if event.actions.is_empty() {
                result.add(ValidationIssue::warning(
                    ValidationCode::NoEventActions,
                    "Event has no actions",
                    format!("{}.actions", path),
                ));
            }

            for (action_idx, action) in event.actions.iter().enumerate() {
                self.validate_action(result, action, format!("{}.actions[{}]", path, action_idx));
            }
        }
    }

    /// Validate trigger.
    fn validate_trigger(&self, result: &mut ValidationResult, trigger: &EventTrigger, path: String) {
        match trigger {
            EventTrigger::Time { at_secs } => {
                if *at_secs < 0.0 {
                    result.add(ValidationIssue::error(
                        ValidationCode::InvalidTriggerTime,
                        format!("Trigger time cannot be negative, got {}", at_secs),
                        &path,
                    ));
                }
            }

            EventTrigger::Periodic { interval_secs, start_secs } => {
                if *interval_secs <= 0.0 {
                    result.add(ValidationIssue::error(
                        ValidationCode::InvalidTriggerInterval,
                        format!("Trigger interval must be positive, got {}", interval_secs),
                        &path,
                    ));
                }
                if *start_secs < 0.0 {
                    result.add(ValidationIssue::warning(
                        ValidationCode::InvalidTriggerTime,
                        format!("Negative start time ({}) will be treated as 0", start_secs),
                        &path,
                    ));
                }
            }

            EventTrigger::Condition { operator, .. } => {
                let valid_ops = ["==", "=", "eq", "!=", "<>", "ne", "<", "lt", "<=", "le", ">", "gt", ">=", "ge"];
                if !valid_ops.contains(&operator.as_str()) {
                    result.add(ValidationIssue::error(
                        ValidationCode::InvalidConditionOperator,
                        format!("Invalid condition operator: '{}'. Valid: {:?}", operator, valid_ops),
                        &path,
                    ));
                }
            }
        }
    }

    /// Validate action.
    fn validate_action(&self, result: &mut ValidationResult, action: &EventAction, path: String) {
        match action {
            EventAction::SetValue { point, .. } => {
                if point.is_empty() {
                    result.add(ValidationIssue::error(
                        ValidationCode::EventTargetNotFound,
                        "SetValue target point cannot be empty",
                        &path,
                    ));
                }
            }

            EventAction::ChangePattern { point, pattern } => {
                if point.is_empty() {
                    result.add(ValidationIssue::error(
                        ValidationCode::EventTargetNotFound,
                        "ChangePattern target point cannot be empty",
                        &path,
                    ));
                }
                self.validate_pattern(result, pattern, format!("{}.pattern", path));
            }

            EventAction::Log { .. } | EventAction::Pause | EventAction::Stop => {
                // No validation needed
            }
        }
    }

    /// Validate cross-references.
    fn validate_references(&self, result: &mut ValidationResult, scenario: &Scenario) {
        // Collect all point IDs
        let point_ids: HashSet<&str> = scenario.points.iter().map(|p| p.id.as_str()).collect();

        // Check Follow sources
        if self.options.check_follow_sources {
            for (idx, point) in scenario.points.iter().enumerate() {
                if let PatternConfig::Follow { source, .. } = &point.pattern {
                    if !point_ids.contains(source.as_str()) {
                        result.add(ValidationIssue::error(
                            ValidationCode::FollowSourceNotFound,
                            format!("Follow source '{}' not found", source),
                            format!("points[{}].pattern", idx),
                        ));
                    }
                }
            }
        }

        // Check circular dependencies
        if self.options.check_circular_deps {
            self.check_circular_dependencies(result, scenario);
        }

        // Check event action targets
        for (event_idx, event) in scenario.events.iter().enumerate() {
            for (action_idx, action) in event.actions.iter().enumerate() {
                let target = match action {
                    EventAction::SetValue { point, .. } => Some(point),
                    EventAction::ChangePattern { point, .. } => Some(point),
                    _ => None,
                };

                if let Some(target) = target {
                    if !target.is_empty() && !point_ids.contains(target.as_str()) {
                        result.add(ValidationIssue::warning(
                            ValidationCode::EventTargetNotFound,
                            format!("Event action target '{}' not found in points", target),
                            format!("events[{}].actions[{}]", event_idx, action_idx),
                        ));
                    }
                }
            }

            // Check condition trigger point
            if let EventTrigger::Condition { point, .. } = &event.trigger {
                if !point_ids.contains(point.as_str()) {
                    result.add(ValidationIssue::warning(
                        ValidationCode::EventTargetNotFound,
                        format!("Condition trigger point '{}' not found", point),
                        format!("events[{}].trigger", event_idx),
                    ));
                }
            }
        }
    }

    /// Check for circular dependencies in Follow patterns.
    fn check_circular_dependencies(&self, result: &mut ValidationResult, scenario: &Scenario) {
        // Build dependency graph
        let mut deps: HashMap<&str, &str> = HashMap::new();

        for point in &scenario.points {
            if let PatternConfig::Follow { source, .. } = &point.pattern {
                deps.insert(&point.id, source);
            }
        }

        // Check for cycles using DFS
        for start in deps.keys() {
            let mut visited = HashSet::new();
            let mut current = *start;

            while let Some(&next) = deps.get(current) {
                if next == *start {
                    // Found cycle back to start
                    result.add(ValidationIssue::error(
                        ValidationCode::FollowCircularDependency,
                        format!("Circular dependency detected starting from '{}'", start),
                        "points",
                    ));
                    break;
                }

                if visited.contains(next) {
                    break; // Already checked this path
                }

                visited.insert(current);
                current = next;
            }
        }
    }

    /// Validate performance implications.
    fn validate_performance(&self, result: &mut ValidationResult, scenario: &Scenario) {
        // Check point count
        if scenario.points.len() > self.options.max_recommended_points {
            result.add(ValidationIssue::warning(
                ValidationCode::PerformanceWarning,
                format!(
                    "Large number of points ({}) may impact performance",
                    scenario.points.len()
                ),
                "points",
            ));
        }

        // Check event count
        if scenario.events.len() > self.options.max_recommended_events {
            result.add(ValidationIssue::warning(
                ValidationCode::PerformanceWarning,
                format!(
                    "Large number of events ({}) may impact performance",
                    scenario.events.len()
                ),
                "events",
            ));
        }

        // Check for very small intervals
        let min_interval = scenario.points.iter().map(|p| p.interval_ms).min();
        if let Some(interval) = min_interval {
            if interval < 10 {
                result.add(ValidationIssue::warning(
                    ValidationCode::PerformanceWarning,
                    format!("Very small interval ({}ms) may cause high CPU usage", interval),
                    "points",
                ));
            }
        }
    }
}

impl Default for ScenarioValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ScenarioEvent, ScenarioPoint};

    fn create_valid_scenario() -> Scenario {
        Scenario {
            name: "Test Scenario".to_string(),
            description: "A test".to_string(),
            duration_secs: 60,
            time_scale: 1.0,
            devices: vec![],
            points: vec![
                ScenarioPoint {
                    id: "temp".to_string(),
                    device_id: "device-1".to_string(),
                    point_id: "temperature".to_string(),
                    pattern: PatternConfig::Constant { value: 25.0 },
                    interval_ms: 1000,
                    device_tags: Tags::new(),
                },
            ],
            events: vec![],
            variables: HashMap::new(),
        }
    }

    #[test]
    fn test_valid_scenario() {
        let scenario = create_valid_scenario();
        let validator = ScenarioValidator::new();
        let result = validator.validate(&scenario);

        assert!(result.is_valid());
        assert_eq!(result.error_count(), 0);
    }

    #[test]
    fn test_empty_name() {
        let mut scenario = create_valid_scenario();
        scenario.name = String::new();

        let validator = ScenarioValidator::new();
        let result = validator.validate(&scenario);

        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.code == ValidationCode::EmptyName));
    }

    #[test]
    fn test_invalid_time_scale() {
        let mut scenario = create_valid_scenario();
        scenario.time_scale = 0.0;

        let validator = ScenarioValidator::new();
        let result = validator.validate(&scenario);

        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.code == ValidationCode::InvalidTimeScale));
    }

    #[test]
    fn test_duplicate_point_id() {
        let mut scenario = create_valid_scenario();
        scenario.points.push(ScenarioPoint {
            id: "temp".to_string(), // Duplicate
            device_id: "device-2".to_string(),
            point_id: "temp2".to_string(),
            pattern: PatternConfig::Constant { value: 30.0 },
            interval_ms: 1000,
            device_tags: Tags::new(),
        });

        let validator = ScenarioValidator::new();
        let result = validator.validate(&scenario);

        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.code == ValidationCode::DuplicatePointId));
    }

    #[test]
    fn test_invalid_sine_period() {
        let mut scenario = create_valid_scenario();
        scenario.points[0].pattern = PatternConfig::Sine {
            amplitude: 10.0,
            offset: 20.0,
            period_secs: 0.0, // Invalid
            phase: 0.0,
        };

        let validator = ScenarioValidator::new();
        let result = validator.validate(&scenario);

        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.code == ValidationCode::InvalidPeriod));
    }

    #[test]
    fn test_follow_source_not_found() {
        let mut scenario = create_valid_scenario();
        scenario.points.push(ScenarioPoint {
            id: "follower".to_string(),
            device_id: "device-1".to_string(),
            point_id: "follower".to_string(),
            pattern: PatternConfig::Follow {
                source: "nonexistent".to_string(),
                offset: 0.0,
                gain: 1.0,
                delay_ms: 0,
            },
            interval_ms: 1000,
            device_tags: Tags::new(),
        });

        let validator = ScenarioValidator::new();
        let result = validator.validate(&scenario);

        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.code == ValidationCode::FollowSourceNotFound));
    }

    #[test]
    fn test_circular_dependency() {
        let mut scenario = create_valid_scenario();
        scenario.points = vec![
            ScenarioPoint {
                id: "a".to_string(),
                device_id: "d1".to_string(),
                point_id: "a".to_string(),
                pattern: PatternConfig::Follow {
                    source: "b".to_string(),
                    offset: 0.0,
                    gain: 1.0,
                    delay_ms: 0,
                },
                interval_ms: 1000,
                device_tags: Tags::new(),
            },
            ScenarioPoint {
                id: "b".to_string(),
                device_id: "d1".to_string(),
                point_id: "b".to_string(),
                pattern: PatternConfig::Follow {
                    source: "a".to_string(), // Circular!
                    offset: 0.0,
                    gain: 1.0,
                    delay_ms: 0,
                },
                interval_ms: 1000,
                device_tags: Tags::new(),
            },
        ];

        let validator = ScenarioValidator::new();
        let result = validator.validate(&scenario);

        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.code == ValidationCode::FollowCircularDependency));
    }

    #[test]
    fn test_invalid_condition_operator() {
        let mut scenario = create_valid_scenario();
        scenario.events.push(ScenarioEvent {
            name: "test".to_string(),
            trigger: EventTrigger::Condition {
                point: "temp".to_string(),
                operator: "invalid".to_string(),
                value: 50.0,
            },
            actions: vec![EventAction::Log {
                message: "test".to_string(),
                level: "info".to_string(),
            }],
        });

        let validator = ScenarioValidator::new();
        let result = validator.validate(&scenario);

        assert!(!result.is_valid());
        assert!(result.errors().iter().any(|e| e.code == ValidationCode::InvalidConditionOperator));
    }

    #[test]
    fn test_performance_warning() {
        let mut scenario = create_valid_scenario();
        scenario.points[0].interval_ms = 1; // Very small

        let validator = ScenarioValidator::new();
        let result = validator.validate(&scenario);

        assert!(result.is_valid()); // Warnings don't make it invalid
        assert!(result.warnings().iter().any(|e| e.code == ValidationCode::PerformanceWarning));
    }
}
