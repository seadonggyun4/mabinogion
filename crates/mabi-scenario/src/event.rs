//! Event system with triggers and actions.
//!
//! Provides a flexible event-driven system for scenarios with:
//! - Time-based triggers (at specific time, periodic)
//! - Condition-based triggers (value comparisons)
//! - Various actions (set value, change pattern, log, pause, stop)

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, error, info, warn};

use crate::schema::{EventAction, EventTrigger, PatternConfig, ScenarioEvent};
use crate::ScenarioResult;

/// Comparison operator for condition triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonOperator {
    Equal,
    NotEqual,
    LessThan,
    LessOrEqual,
    GreaterThan,
    GreaterOrEqual,
}

impl ComparisonOperator {
    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim() {
            "==" | "=" | "eq" => Some(Self::Equal),
            "!=" | "<>" | "ne" => Some(Self::NotEqual),
            "<" | "lt" => Some(Self::LessThan),
            "<=" | "le" => Some(Self::LessOrEqual),
            ">" | "gt" => Some(Self::GreaterThan),
            ">=" | "ge" => Some(Self::GreaterOrEqual),
            _ => None,
        }
    }

    /// Evaluate comparison.
    pub fn evaluate(&self, left: f64, right: f64) -> bool {
        const EPSILON: f64 = 1e-9;
        match self {
            Self::Equal => (left - right).abs() < EPSILON,
            Self::NotEqual => (left - right).abs() >= EPSILON,
            Self::LessThan => left < right,
            Self::LessOrEqual => left <= right,
            Self::GreaterThan => left > right,
            Self::GreaterOrEqual => left >= right,
        }
    }
}

/// Trigger state.
#[derive(Debug, Clone)]
pub enum TriggerState {
    /// Time-based trigger.
    Time {
        at_secs: f64,
        fired: bool,
    },
    /// Periodic trigger.
    Periodic {
        interval_secs: f64,
        start_secs: f64,
        last_fire_secs: f64,
    },
    /// Condition-based trigger.
    Condition {
        point: String,
        operator: ComparisonOperator,
        value: f64,
        /// Previous condition state (for edge detection).
        was_true: bool,
    },
}

impl TriggerState {
    /// Create from schema trigger.
    pub fn from_schema(trigger: &EventTrigger) -> Option<Self> {
        match trigger {
            EventTrigger::Time { at_secs } => Some(Self::Time {
                at_secs: *at_secs,
                fired: false,
            }),
            EventTrigger::Periodic {
                interval_secs,
                start_secs,
            } => Some(Self::Periodic {
                interval_secs: *interval_secs,
                start_secs: *start_secs,
                last_fire_secs: *start_secs - *interval_secs, // So first fire happens at start_secs
            }),
            EventTrigger::Condition {
                point,
                operator,
                value,
            } => {
                let op = ComparisonOperator::from_str(operator)?;
                Some(Self::Condition {
                    point: point.clone(),
                    operator: op,
                    value: *value,
                    was_true: false,
                })
            }
        }
    }

    /// Reset trigger state.
    pub fn reset(&mut self) {
        match self {
            Self::Time { fired, .. } => *fired = false,
            Self::Periodic {
                last_fire_secs,
                start_secs,
                interval_secs,
            } => {
                *last_fire_secs = *start_secs - *interval_secs;
            }
            Self::Condition { was_true, .. } => *was_true = false,
        }
    }
}

/// Action to be executed.
#[derive(Debug, Clone)]
pub enum ActionCommand {
    /// Set a point value.
    SetValue { point: String, value: f64 },
    /// Change a point's pattern.
    ChangePattern { point: String, pattern: PatternConfig },
    /// Log a message.
    Log { message: String, level: LogLevel },
    /// Pause the scenario.
    Pause,
    /// Stop the scenario.
    Stop,
}

/// Log level for Log action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// Parse from string.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "debug" => Self::Debug,
            "warn" | "warning" => Self::Warn,
            "error" => Self::Error,
            _ => Self::Info,
        }
    }
}

impl ActionCommand {
    /// Create from schema action.
    pub fn from_schema(action: &EventAction) -> Self {
        match action {
            EventAction::SetValue { point, value } => Self::SetValue {
                point: point.clone(),
                value: *value,
            },
            EventAction::ChangePattern { point, pattern } => Self::ChangePattern {
                point: point.clone(),
                pattern: pattern.clone(),
            },
            EventAction::Log { message, level } => Self::Log {
                message: message.clone(),
                level: LogLevel::from_str(level),
            },
            EventAction::Pause => Self::Pause,
            EventAction::Stop => Self::Stop,
        }
    }
}

/// Event instance with state.
pub struct EventInstance {
    /// Event name.
    pub name: String,
    /// Trigger state.
    trigger: TriggerState,
    /// Actions to execute.
    actions: Vec<ActionCommand>,
    /// Whether this event is enabled.
    enabled: bool,
    /// Fire count.
    fire_count: u64,
}

impl EventInstance {
    /// Create from schema event.
    pub fn from_schema(event: &ScenarioEvent) -> Option<Self> {
        let trigger = TriggerState::from_schema(&event.trigger)?;
        let actions = event
            .actions
            .iter()
            .map(ActionCommand::from_schema)
            .collect();

        Some(Self {
            name: event.name.clone(),
            trigger,
            actions,
            enabled: true,
            fire_count: 0,
        })
    }

    /// Check if trigger should fire.
    pub fn check_trigger(&mut self, elapsed_secs: f64, point_values: &HashMap<String, f64>) -> bool {
        if !self.enabled {
            return false;
        }

        match &mut self.trigger {
            TriggerState::Time { at_secs, fired } => {
                if !*fired && elapsed_secs >= *at_secs {
                    *fired = true;
                    true
                } else {
                    false
                }
            }
            TriggerState::Periodic {
                interval_secs,
                start_secs,
                last_fire_secs,
            } => {
                if elapsed_secs < *start_secs {
                    return false;
                }

                let next_fire = *last_fire_secs + *interval_secs;
                if elapsed_secs >= next_fire {
                    *last_fire_secs = elapsed_secs;
                    true
                } else {
                    false
                }
            }
            TriggerState::Condition {
                point,
                operator,
                value,
                was_true,
            } => {
                let current_value = point_values.get(point).copied().unwrap_or(0.0);
                let is_true = operator.evaluate(current_value, *value);

                // Edge detection: fire only on rising edge (false -> true)
                if is_true && !*was_true {
                    *was_true = true;
                    true
                } else {
                    *was_true = is_true;
                    false
                }
            }
        }
    }

    /// Get actions.
    pub fn actions(&self) -> &[ActionCommand] {
        &self.actions
    }

    /// Enable/disable event.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Check if enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get fire count.
    pub fn fire_count(&self) -> u64 {
        self.fire_count
    }

    /// Increment fire count.
    pub fn increment_fire_count(&mut self) {
        self.fire_count += 1;
    }

    /// Reset state.
    pub fn reset(&mut self) {
        self.trigger.reset();
        self.fire_count = 0;
    }
}

/// Action result from event processing.
#[derive(Debug, Clone)]
pub struct ActionResult {
    /// Event name.
    pub event_name: String,
    /// Action command.
    pub action: ActionCommand,
}

/// Event context for action execution.
pub trait EventContext {
    /// Set a point value.
    fn set_value(&mut self, point: &str, value: f64);

    /// Change a point's pattern.
    fn change_pattern(&mut self, point: &str, pattern: PatternConfig);

    /// Pause the scenario.
    fn pause(&mut self);

    /// Stop the scenario.
    fn stop(&mut self);
}

/// Event manager for processing scenario events.
pub struct EventManager {
    /// Event instances.
    events: Vec<EventInstance>,
    /// Point values for condition evaluation.
    point_values: Arc<RwLock<HashMap<String, f64>>>,
}

impl EventManager {
    /// Create a new event manager.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            point_values: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with shared point values.
    pub fn with_point_values(point_values: Arc<RwLock<HashMap<String, f64>>>) -> Self {
        Self {
            events: Vec::new(),
            point_values,
        }
    }

    /// Register events from schema.
    pub fn register_events(&mut self, events: &[ScenarioEvent]) {
        for event in events {
            if let Some(instance) = EventInstance::from_schema(event) {
                self.events.push(instance);
            } else {
                warn!(event = %event.name, "Failed to parse event trigger");
            }
        }
    }

    /// Register a single event.
    pub fn register(&mut self, event: EventInstance) {
        self.events.push(event);
    }

    /// Update a point value for condition evaluation.
    pub fn update_point_value(&self, point: &str, value: f64) {
        self.point_values.write().insert(point.to_string(), value);
    }

    /// Process events at current time.
    pub fn process(&mut self, elapsed_secs: f64) -> Vec<ActionResult> {
        let values = self.point_values.read().clone();
        let mut results = Vec::new();

        for event in &mut self.events {
            if event.check_trigger(elapsed_secs, &values) {
                event.increment_fire_count();
                debug!(event = %event.name, fire_count = event.fire_count(), "Event triggered");

                for action in event.actions().iter().cloned() {
                    results.push(ActionResult {
                        event_name: event.name.clone(),
                        action,
                    });
                }
            }
        }

        results
    }

    /// Execute action results.
    pub fn execute_actions<C: EventContext>(
        &self,
        results: &[ActionResult],
        context: &mut C,
    ) -> ScenarioResult<()> {
        for result in results {
            match &result.action {
                ActionCommand::SetValue { point, value } => {
                    context.set_value(point, *value);
                    debug!(event = %result.event_name, point = %point, value = value, "Set value");
                }
                ActionCommand::ChangePattern { point, pattern } => {
                    context.change_pattern(point, pattern.clone());
                    debug!(event = %result.event_name, point = %point, "Changed pattern");
                }
                ActionCommand::Log { message, level } => {
                    match level {
                        LogLevel::Debug => debug!(event = %result.event_name, "{}", message),
                        LogLevel::Info => info!(event = %result.event_name, "{}", message),
                        LogLevel::Warn => warn!(event = %result.event_name, "{}", message),
                        LogLevel::Error => error!(event = %result.event_name, "{}", message),
                    }
                }
                ActionCommand::Pause => {
                    context.pause();
                    info!(event = %result.event_name, "Scenario paused");
                }
                ActionCommand::Stop => {
                    context.stop();
                    info!(event = %result.event_name, "Scenario stopped");
                }
            }
        }

        Ok(())
    }

    /// Reset all event states.
    pub fn reset(&mut self) {
        for event in &mut self.events {
            event.reset();
        }
        self.point_values.write().clear();
    }

    /// Get event by name.
    pub fn get_event(&self, name: &str) -> Option<&EventInstance> {
        self.events.iter().find(|e| e.name == name)
    }

    /// Get mutable event by name.
    pub fn get_event_mut(&mut self, name: &str) -> Option<&mut EventInstance> {
        self.events.iter_mut().find(|e| e.name == name)
    }

    /// Enable/disable event by name.
    pub fn set_event_enabled(&mut self, name: &str, enabled: bool) -> bool {
        if let Some(event) = self.get_event_mut(name) {
            event.set_enabled(enabled);
            true
        } else {
            false
        }
    }

    /// Get shared point values.
    pub fn point_values(&self) -> Arc<RwLock<HashMap<String, f64>>> {
        Arc::clone(&self.point_values)
    }

    /// Get event count.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

impl Default for EventManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating events programmatically.
pub struct EventBuilder {
    name: String,
    trigger: Option<TriggerState>,
    actions: Vec<ActionCommand>,
}

impl EventBuilder {
    /// Create a new event builder.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            trigger: None,
            actions: Vec::new(),
        }
    }

    /// Set time trigger.
    pub fn at_time(mut self, secs: f64) -> Self {
        self.trigger = Some(TriggerState::Time {
            at_secs: secs,
            fired: false,
        });
        self
    }

    /// Set periodic trigger.
    pub fn periodic(mut self, interval_secs: f64, start_secs: f64) -> Self {
        self.trigger = Some(TriggerState::Periodic {
            interval_secs,
            start_secs,
            last_fire_secs: start_secs - interval_secs,
        });
        self
    }

    /// Set condition trigger.
    pub fn when(mut self, point: impl Into<String>, operator: &str, value: f64) -> Self {
        if let Some(op) = ComparisonOperator::from_str(operator) {
            self.trigger = Some(TriggerState::Condition {
                point: point.into(),
                operator: op,
                value,
                was_true: false,
            });
        }
        self
    }

    /// Add set value action.
    pub fn set_value(mut self, point: impl Into<String>, value: f64) -> Self {
        self.actions.push(ActionCommand::SetValue {
            point: point.into(),
            value,
        });
        self
    }

    /// Add change pattern action.
    pub fn change_pattern(mut self, point: impl Into<String>, pattern: PatternConfig) -> Self {
        self.actions.push(ActionCommand::ChangePattern {
            point: point.into(),
            pattern,
        });
        self
    }

    /// Add log action.
    pub fn log(mut self, message: impl Into<String>, level: LogLevel) -> Self {
        self.actions.push(ActionCommand::Log {
            message: message.into(),
            level,
        });
        self
    }

    /// Add pause action.
    pub fn pause(mut self) -> Self {
        self.actions.push(ActionCommand::Pause);
        self
    }

    /// Add stop action.
    pub fn stop(mut self) -> Self {
        self.actions.push(ActionCommand::Stop);
        self
    }

    /// Build the event instance.
    pub fn build(self) -> Option<EventInstance> {
        let trigger = self.trigger?;

        Some(EventInstance {
            name: self.name,
            trigger,
            actions: self.actions,
            enabled: true,
            fire_count: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockContext {
        values: HashMap<String, f64>,
        patterns: HashMap<String, PatternConfig>,
        paused: bool,
        stopped: bool,
    }

    impl MockContext {
        fn new() -> Self {
            Self {
                values: HashMap::new(),
                patterns: HashMap::new(),
                paused: false,
                stopped: false,
            }
        }
    }

    impl EventContext for MockContext {
        fn set_value(&mut self, point: &str, value: f64) {
            self.values.insert(point.to_string(), value);
        }

        fn change_pattern(&mut self, point: &str, pattern: PatternConfig) {
            self.patterns.insert(point.to_string(), pattern);
        }

        fn pause(&mut self) {
            self.paused = true;
        }

        fn stop(&mut self) {
            self.stopped = true;
        }
    }

    #[test]
    fn test_comparison_operators() {
        assert!(ComparisonOperator::Equal.evaluate(10.0, 10.0));
        assert!(!ComparisonOperator::Equal.evaluate(10.0, 11.0));

        assert!(ComparisonOperator::NotEqual.evaluate(10.0, 11.0));
        assert!(!ComparisonOperator::NotEqual.evaluate(10.0, 10.0));

        assert!(ComparisonOperator::LessThan.evaluate(10.0, 11.0));
        assert!(!ComparisonOperator::LessThan.evaluate(11.0, 10.0));

        assert!(ComparisonOperator::GreaterOrEqual.evaluate(10.0, 10.0));
        assert!(ComparisonOperator::GreaterOrEqual.evaluate(11.0, 10.0));
    }

    #[test]
    fn test_time_trigger() {
        let event = EventBuilder::new("test")
            .at_time(5.0)
            .log("Triggered!", LogLevel::Info)
            .build()
            .unwrap();

        let mut manager = EventManager::new();
        manager.register(event);

        // Before trigger time
        let results = manager.process(4.0);
        assert!(results.is_empty());

        // At trigger time
        let results = manager.process(5.0);
        assert_eq!(results.len(), 1);

        // After trigger (should not fire again)
        let results = manager.process(6.0);
        assert!(results.is_empty());
    }

    #[test]
    fn test_periodic_trigger() {
        let event = EventBuilder::new("heartbeat")
            .periodic(1.0, 0.0)
            .log("Tick", LogLevel::Debug)
            .build()
            .unwrap();

        let mut manager = EventManager::new();
        manager.register(event);

        // First fire at 0
        let results = manager.process(0.0);
        assert_eq!(results.len(), 1);

        // Second fire at 1.0
        let results = manager.process(1.0);
        assert_eq!(results.len(), 1);

        // No fire at 1.5
        let results = manager.process(1.5);
        assert!(results.is_empty());

        // Third fire at 2.0
        let results = manager.process(2.0);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_condition_trigger() {
        let event = EventBuilder::new("high_temp")
            .when("temperature", ">", 100.0)
            .set_value("alarm", 1.0)
            .build()
            .unwrap();

        let mut manager = EventManager::new();
        manager.register(event);

        // Below threshold
        manager.update_point_value("temperature", 50.0);
        let results = manager.process(0.0);
        assert!(results.is_empty());

        // Above threshold (rising edge)
        manager.update_point_value("temperature", 110.0);
        let results = manager.process(1.0);
        assert_eq!(results.len(), 1);

        // Still above (no trigger - not a rising edge)
        manager.update_point_value("temperature", 120.0);
        let results = manager.process(2.0);
        assert!(results.is_empty());

        // Drop below and rise again
        manager.update_point_value("temperature", 50.0);
        manager.process(3.0);
        manager.update_point_value("temperature", 110.0);
        let results = manager.process(4.0);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_action_execution() {
        let event = EventBuilder::new("test")
            .at_time(0.0)
            .set_value("output", 42.0)
            .pause()
            .build()
            .unwrap();

        let mut manager = EventManager::new();
        manager.register(event);

        let results = manager.process(0.0);
        let mut context = MockContext::new();

        manager.execute_actions(&results, &mut context).unwrap();

        assert_eq!(context.values.get("output"), Some(&42.0));
        assert!(context.paused);
    }

    #[test]
    fn test_event_enable_disable() {
        let event = EventBuilder::new("test")
            .at_time(0.0)
            .log("Test", LogLevel::Info)
            .build()
            .unwrap();

        let mut manager = EventManager::new();
        manager.register(event);

        // Disable event
        manager.set_event_enabled("test", false);
        let results = manager.process(0.0);
        assert!(results.is_empty());

        // Reset and enable
        manager.reset();
        manager.set_event_enabled("test", true);
        let results = manager.process(0.0);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_event_from_schema() {
        let schema_event = ScenarioEvent {
            name: "alarm".to_string(),
            trigger: EventTrigger::Condition {
                point: "temp".to_string(),
                operator: ">=".to_string(),
                value: 50.0,
            },
            actions: vec![
                EventAction::SetValue {
                    point: "alarm_flag".to_string(),
                    value: 1.0,
                },
                EventAction::Log {
                    message: "High temperature!".to_string(),
                    level: "warn".to_string(),
                },
            ],
        };

        let instance = EventInstance::from_schema(&schema_event).unwrap();
        assert_eq!(instance.name, "alarm");
        assert_eq!(instance.actions.len(), 2);
    }
}
