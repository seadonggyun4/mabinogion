//! Scenario player.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::broadcast;
use tracing::info;

use mabi_core::types::DataPointId;
use mabi_core::value::Value;

use crate::generator::PatternGenerator;
use crate::schema::Scenario;
use crate::ScenarioResult;

/// Player configuration.
#[derive(Debug, Clone)]
#[derive(Default)]
pub struct PlayerConfig {
    /// Time scale override (None = use scenario default).
    pub time_scale: Option<f64>,

    /// Maximum duration override.
    pub max_duration: Option<Duration>,
}


/// Value update event.
#[derive(Debug, Clone)]
pub struct ValueUpdate {
    pub point_id: DataPointId,
    pub value: Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Scenario player state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Stopped,
    Running,
    Paused,
    Completed,
}

/// Scenario player.
pub struct ScenarioPlayer {
    scenario: Scenario,
    config: PlayerConfig,
    generators: HashMap<String, PatternGenerator>,
    state: PlayerState,
    start_time: Option<Instant>,
    elapsed_time: Duration,
    update_tx: broadcast::Sender<ValueUpdate>,
    pause_flag: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
}

impl ScenarioPlayer {
    /// Create a new scenario player.
    pub fn new(scenario: Scenario, config: PlayerConfig) -> Self {
        let (update_tx, _) = broadcast::channel(10_000);

        // Create generators for each point
        let mut generators = HashMap::new();
        for point in &scenario.points {
            let gen = PatternGenerator::new(point.pattern.clone());
            generators.insert(point.id.clone(), gen);
        }

        Self {
            scenario,
            config,
            generators,
            state: PlayerState::Stopped,
            start_time: None,
            elapsed_time: Duration::ZERO,
            update_tx,
            pause_flag: Arc::new(AtomicBool::new(false)),
            stop_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get current state.
    pub fn state(&self) -> PlayerState {
        self.state
    }

    /// Get scenario.
    pub fn scenario(&self) -> &Scenario {
        &self.scenario
    }

    /// Subscribe to value updates.
    pub fn subscribe(&self) -> broadcast::Receiver<ValueUpdate> {
        self.update_tx.subscribe()
    }

    /// Get elapsed time.
    pub fn elapsed(&self) -> Duration {
        if let Some(start) = self.start_time {
            if self.state == PlayerState::Running {
                self.elapsed_time + start.elapsed()
            } else {
                self.elapsed_time
            }
        } else {
            Duration::ZERO
        }
    }

    /// Start or resume the scenario.
    pub fn start(&mut self) {
        if self.state == PlayerState::Stopped || self.state == PlayerState::Paused {
            self.start_time = Some(Instant::now());
            self.state = PlayerState::Running;
            self.pause_flag.store(false, Ordering::SeqCst);
            info!(scenario = %self.scenario.name, "Scenario started");
        }
    }

    /// Pause the scenario.
    pub fn pause(&mut self) {
        if self.state == PlayerState::Running {
            if let Some(start) = self.start_time {
                self.elapsed_time += start.elapsed();
            }
            self.state = PlayerState::Paused;
            self.pause_flag.store(true, Ordering::SeqCst);
            info!(scenario = %self.scenario.name, "Scenario paused");
        }
    }

    /// Stop the scenario.
    pub fn stop(&mut self) {
        self.state = PlayerState::Stopped;
        self.start_time = None;
        self.elapsed_time = Duration::ZERO;
        self.stop_flag.store(true, Ordering::SeqCst);

        // Reset generators
        for gen in self.generators.values_mut() {
            gen.reset();
        }

        info!(scenario = %self.scenario.name, "Scenario stopped");
    }

    /// Run the scenario (blocking until complete or stopped).
    pub async fn run(&mut self) -> ScenarioResult<()> {
        self.start();

        let time_scale = self.config.time_scale.unwrap_or(self.scenario.time_scale);
        let duration = if self.scenario.duration_secs > 0 {
            Some(Duration::from_secs_f64(
                self.scenario.duration_secs as f64 / time_scale,
            ))
        } else {
            self.config.max_duration
        };

        while self.state == PlayerState::Running {
            // Check stop flag
            if self.stop_flag.load(Ordering::SeqCst) {
                break;
            }

            // Check pause flag
            if self.pause_flag.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }

            // Check duration
            if let Some(max_duration) = duration {
                if self.elapsed() >= max_duration {
                    self.state = PlayerState::Completed;
                    break;
                }
            }

            // Generate and send updates
            self.tick().await?;

            // Wait for next tick (adjust for time scale)
            let min_interval = self
                .scenario
                .points
                .iter()
                .map(|p| p.interval_ms)
                .min()
                .unwrap_or(1000);

            let sleep_duration = Duration::from_millis((min_interval as f64 / time_scale) as u64);

            tokio::time::sleep(sleep_duration).await;
        }

        if self.state == PlayerState::Completed {
            info!(scenario = %self.scenario.name, "Scenario completed");
        }

        Ok(())
    }

    /// Process one tick.
    pub async fn tick(&mut self) -> ScenarioResult<()> {
        let elapsed_secs = self.elapsed().as_secs_f64()
            * self.config.time_scale.unwrap_or(self.scenario.time_scale);

        for point in &self.scenario.points {
            if let Some(gen) = self.generators.get_mut(&point.id) {
                let value = gen.generate_at(elapsed_secs);

                let update = ValueUpdate {
                    point_id: DataPointId::new(&point.device_id, &point.point_id),
                    value: Value::F64(value),
                    timestamp: chrono::Utc::now(),
                };

                let _ = self.update_tx.send(update);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{PatternConfig, ScenarioPoint};

    #[tokio::test]
    async fn test_scenario_player() {
        let scenario = Scenario {
            name: "Test".to_string(),
            duration_secs: 1,
            time_scale: 10.0, // 10x speed
            points: vec![ScenarioPoint {
                id: "temp".to_string(),
                device_id: "device-001".to_string(),
                point_id: "temperature".to_string(),
                pattern: PatternConfig::Constant { value: 25.0 },
                interval_ms: 100,
            }],
            ..Default::default()
        };

        let config = PlayerConfig::default();
        let mut player = ScenarioPlayer::new(scenario, config);
        let mut rx = player.subscribe();

        // Start player in background
        let player_handle = tokio::spawn(async move { player.run().await });

        // Receive some updates
        let update = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await;

        assert!(update.is_ok());
        let update = update.unwrap().unwrap();
        assert!((update.value.as_f64().unwrap() - 25.0).abs() < 0.001);

        player_handle.await.unwrap().unwrap();
    }
}
