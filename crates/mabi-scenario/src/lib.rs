//! # mabi-scenario
//!
//! Scenario engine for the OTSIM protocol simulator.
//!
//! This crate provides:
//! - Scenario definition and parsing (YAML/JSON)
//! - Pattern generators (Sine, Ramp, Step, Random, Follow, Replay)
//! - Scenario player with time scaling
//! - Event triggers and conditions
//! - Comprehensive schema validation
//! - Industrial scenario templates
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Scenario Engine                          │
//! │       (Parser, Validator, Player, Event Manager)            │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!           ┌──────────────────┼──────────────────┐
//!           ▼                  ▼                  ▼
//! ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
//! │    Patterns     │ │     Events      │ │    Templates    │
//! │ (Generator)     │ │ (Triggers/Acts) │ │ (Industrial)    │
//! └─────────────────┘ └─────────────────┘ └─────────────────┘
//!           │                  │
//!           ▼                  ▼
//! ┌─────────────────┐ ┌─────────────────┐
//! │  Follow/Replay  │ │   Validation    │
//! │ (Advanced)      │ │ (Schema Check)  │
//! └─────────────────┘ └─────────────────┘
//! ```
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use mabi_scenario::prelude::*;
//!
//! // Load scenario from file
//! let scenario = ScenarioParser::parse_file("scenario.yaml")?;
//!
//! // Validate
//! let validator = ScenarioValidator::new();
//! let result = validator.validate(&scenario);
//! if !result.is_valid() {
//!     for error in result.errors() {
//!         eprintln!("{}: {}", error.path, error.message);
//!     }
//! }
//!
//! // Create player and run
//! let mut player = ScenarioPlayer::new(scenario, PlayerConfig::default());
//! let mut rx = player.subscribe();
//!
//! tokio::spawn(async move {
//!     player.run().await;
//! });
//!
//! // Process updates
//! while let Ok(update) = rx.recv().await {
//!     println!("{}: {}", update.point_id, update.value);
//! }
//! ```

pub mod event;
pub mod executor;
pub mod follow;
pub mod generator;
pub mod parser;
pub mod player;
pub mod replay;
pub mod schema;
pub mod templates;
pub mod validation;

pub use generator::{PatternGenerator, PatternType};
pub use parser::ScenarioParser;
pub use player::{PlayerConfig, ScenarioPlayer};
pub use schema::{Scenario, ScenarioEvent, ScenarioPoint};

// Prelude for common imports
pub mod prelude {
    pub use crate::event::{
        ActionCommand, ActionResult, ComparisonOperator, EventBuilder, EventContext,
        EventInstance, EventManager, LogLevel,
    };
    pub use crate::executor::{
        DeviceRegistry, ExecutorBuilder, ExecutorConfig, ExecutorMetrics, ExecutorState,
        ScenarioExecutor,
    };
    pub use crate::follow::{
        DelayBuffer, FollowConfig, FollowManager, FollowState, SourceRegistry, TimestampedValue,
    };
    pub use crate::generator::{PatternGenerator, PatternType};
    pub use crate::parser::ScenarioParser;
    pub use crate::player::{PlayerConfig, PlayerState, ScenarioPlayer, ValueUpdate};
    pub use crate::replay::{
        InterpolationMethod, ReplayConfig, ReplayDataPoint, ReplayLoader, ReplayManager,
        ReplaySeries, ReplayState,
    };
    pub use crate::schema::{
        EventAction, EventTrigger, PatternConfig, Scenario, ScenarioEvent, ScenarioPoint,
    };
    pub use crate::templates::{
        Template, TemplateCategory, TemplateInfo, TemplateOptions, TemplateRegistry,
    };
    pub use crate::validation::{
        ScenarioValidator, ValidationCode, ValidationIssue, ValidationOptions, ValidationResult,
        ValidationSeverity,
    };
    pub use crate::{ScenarioError, ScenarioResult};
}

use thiserror::Error;
use mabi_core::Error as CoreError;

/// Scenario result type.
pub type ScenarioResult<T> = Result<T, ScenarioError>;

/// Scenario error types.
#[derive(Debug, Error)]
pub enum ScenarioError {
    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Invalid pattern: {0}")]
    InvalidPattern(String),

    #[error("Scenario not found: {0}")]
    NotFound(String),

    #[error("Player error: {0}")]
    Player(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Core error: {0}")]
    Core(#[from] CoreError),
}
