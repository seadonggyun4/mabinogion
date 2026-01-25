//! Industrial scenario templates.
//!
//! Pre-built templates for common industrial scenarios:
//! - HVAC systems (Building automation)
//! - Manufacturing processes
//! - Power/energy monitoring
//! - Water treatment
//! - Data center monitoring

use std::collections::HashMap;

use crate::schema::{EventAction, EventTrigger, PatternConfig, Scenario, ScenarioEvent, ScenarioPoint};

/// Template category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateCategory {
    /// Building automation (HVAC, lighting, access)
    BuildingAutomation,
    /// Manufacturing and process control
    Manufacturing,
    /// Power and energy systems
    Energy,
    /// Water and utilities
    Water,
    /// Data center infrastructure
    DataCenter,
}

/// Template metadata.
#[derive(Debug, Clone)]
pub struct TemplateInfo {
    /// Template ID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Category.
    pub category: TemplateCategory,
    /// Supported protocols.
    pub protocols: Vec<String>,
    /// Number of devices.
    pub device_count: usize,
    /// Number of data points.
    pub point_count: usize,
}

/// Scenario template.
pub struct Template {
    /// Template metadata.
    pub info: TemplateInfo,
    /// Generate scenario function.
    generator: Box<dyn Fn(TemplateOptions) -> Scenario + Send + Sync>,
}

impl Template {
    /// Create a new template.
    pub fn new<F>(info: TemplateInfo, generator: F) -> Self
    where
        F: Fn(TemplateOptions) -> Scenario + Send + Sync + 'static,
    {
        Self {
            info,
            generator: Box::new(generator),
        }
    }

    /// Generate scenario from template.
    pub fn generate(&self, options: TemplateOptions) -> Scenario {
        (self.generator)(options)
    }
}

/// Template generation options.
#[derive(Debug, Clone)]
pub struct TemplateOptions {
    /// Number of devices to generate.
    pub device_count: usize,
    /// Time scale.
    pub time_scale: f64,
    /// Duration in seconds.
    pub duration_secs: u64,
    /// Update interval in milliseconds.
    pub interval_ms: u64,
    /// Custom variables.
    pub variables: HashMap<String, f64>,
}

impl Default for TemplateOptions {
    fn default() -> Self {
        Self {
            device_count: 10,
            time_scale: 1.0,
            duration_secs: 3600,
            interval_ms: 1000,
            variables: HashMap::new(),
        }
    }
}

impl TemplateOptions {
    /// Create with device count.
    pub fn with_devices(mut self, count: usize) -> Self {
        self.device_count = count;
        self
    }

    /// Set time scale.
    pub fn with_time_scale(mut self, scale: f64) -> Self {
        self.time_scale = scale;
        self
    }

    /// Set duration.
    pub fn with_duration(mut self, secs: u64) -> Self {
        self.duration_secs = secs;
        self
    }

    /// Set interval.
    pub fn with_interval(mut self, ms: u64) -> Self {
        self.interval_ms = ms;
        self
    }
}

/// Template registry.
pub struct TemplateRegistry {
    templates: HashMap<String, Template>,
}

impl TemplateRegistry {
    /// Create a new registry with built-in templates.
    pub fn new() -> Self {
        let mut registry = Self {
            templates: HashMap::new(),
        };

        // Register built-in templates
        registry.register(create_hvac_template());
        registry.register(create_manufacturing_template());
        registry.register(create_energy_template());
        registry.register(create_water_template());
        registry.register(create_datacenter_template());

        registry
    }

    /// Register a template.
    pub fn register(&mut self, template: Template) {
        self.templates.insert(template.info.id.clone(), template);
    }

    /// Get template by ID.
    pub fn get(&self, id: &str) -> Option<&Template> {
        self.templates.get(id)
    }

    /// List all templates.
    pub fn list(&self) -> Vec<&TemplateInfo> {
        self.templates.values().map(|t| &t.info).collect()
    }

    /// List templates by category.
    pub fn list_by_category(&self, category: TemplateCategory) -> Vec<&TemplateInfo> {
        self.templates
            .values()
            .filter(|t| t.info.category == category)
            .map(|t| &t.info)
            .collect()
    }

    /// Generate scenario from template.
    pub fn generate(&self, id: &str, options: TemplateOptions) -> Option<Scenario> {
        self.templates.get(id).map(|t| t.generate(options))
    }
}

impl Default for TemplateRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Built-in Templates
// ============================================================================

/// Create HVAC template.
fn create_hvac_template() -> Template {
    let info = TemplateInfo {
        id: "hvac-building".to_string(),
        name: "HVAC Building Automation".to_string(),
        description: "Simulates a building HVAC system with multiple zones, AHUs, and VAV boxes".to_string(),
        category: TemplateCategory::BuildingAutomation,
        protocols: vec!["BACnet".to_string(), "Modbus".to_string()],
        device_count: 10,
        point_count: 50,
    };

    Template::new(info, |options| {
        let mut points = Vec::new();
        let mut events = Vec::new();

        for i in 0..options.device_count {
            let device_id = format!("ahu-{:03}", i + 1);

            // Zone temperature - sine wave with daily cycle
            points.push(ScenarioPoint {
                id: format!("{}-zone-temp", device_id),
                device_id: device_id.clone(),
                point_id: "zone_temperature".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 2.0,
                    offset: 22.0, // 22°C base
                    period_secs: 86400.0 / options.time_scale, // 24-hour cycle
                    phase: (i as f64) * 0.5, // Phase offset per zone
                },
                interval_ms: options.interval_ms,
            });

            // Supply air temperature - ramp with random noise
            points.push(ScenarioPoint {
                id: format!("{}-supply-temp", device_id),
                device_id: device_id.clone(),
                point_id: "supply_air_temperature".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 14.0,
                    std_dev: 0.5,
                },
                interval_ms: options.interval_ms,
            });

            // Damper position - follows setpoint
            points.push(ScenarioPoint {
                id: format!("{}-damper", device_id),
                device_id: device_id.clone(),
                point_id: "damper_position".to_string(),
                pattern: PatternConfig::Ramp {
                    start: 0.0,
                    end: 100.0,
                    duration_secs: 300.0,
                    repeat: true,
                },
                interval_ms: options.interval_ms,
            });

            // Fan speed
            points.push(ScenarioPoint {
                id: format!("{}-fan-speed", device_id),
                device_id: device_id.clone(),
                point_id: "fan_speed".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![0.0, 30.0, 60.0, 100.0],
                    step_duration_secs: 900.0, // 15 min steps
                },
                interval_ms: options.interval_ms,
            });

            // Occupancy
            points.push(ScenarioPoint {
                id: format!("{}-occupancy", device_id),
                device_id: device_id.clone(),
                point_id: "occupancy".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![0.0, 1.0], // Unoccupied / Occupied
                    step_duration_secs: 28800.0, // 8-hour cycle
                },
                interval_ms: options.interval_ms * 10, // Less frequent updates
            });
        }

        // High temperature alarm event
        events.push(ScenarioEvent {
            name: "high-temp-alarm".to_string(),
            trigger: EventTrigger::Condition {
                point: "ahu-001-zone-temp".to_string(),
                operator: ">".to_string(),
                value: 26.0,
            },
            actions: vec![
                EventAction::Log {
                    message: "High temperature alarm triggered".to_string(),
                    level: "warn".to_string(),
                },
            ],
        });

        Scenario {
            name: "HVAC Building Simulation".to_string(),
            description: format!("Simulates {} HVAC units with temperature and airflow control", options.device_count),
            duration_secs: options.duration_secs,
            time_scale: options.time_scale,
            points,
            events,
            variables: options.variables,
        }
    })
}

/// Create manufacturing template.
fn create_manufacturing_template() -> Template {
    let info = TemplateInfo {
        id: "manufacturing-line".to_string(),
        name: "Manufacturing Production Line".to_string(),
        description: "Simulates a production line with conveyors, sensors, and PLCs".to_string(),
        category: TemplateCategory::Manufacturing,
        protocols: vec!["Modbus".to_string(), "OPC UA".to_string()],
        device_count: 20,
        point_count: 100,
    };

    Template::new(info, |options| {
        let mut points = Vec::new();
        let mut events = Vec::new();

        for i in 0..options.device_count {
            let device_id = format!("station-{:03}", i + 1);

            // Conveyor speed
            points.push(ScenarioPoint {
                id: format!("{}-conveyor-speed", device_id),
                device_id: device_id.clone(),
                point_id: "conveyor_speed".to_string(),
                pattern: PatternConfig::Constant { value: 1.5 }, // m/s
                interval_ms: options.interval_ms,
            });

            // Part count
            points.push(ScenarioPoint {
                id: format!("{}-part-count", device_id),
                device_id: device_id.clone(),
                point_id: "part_count".to_string(),
                pattern: PatternConfig::Ramp {
                    start: 0.0,
                    end: 1000.0,
                    duration_secs: 3600.0,
                    repeat: false,
                },
                interval_ms: options.interval_ms,
            });

            // Motor current
            points.push(ScenarioPoint {
                id: format!("{}-motor-current", device_id),
                device_id: device_id.clone(),
                point_id: "motor_current".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 5.2,
                    std_dev: 0.3,
                },
                interval_ms: options.interval_ms / 2,
            });

            // Proximity sensor
            points.push(ScenarioPoint {
                id: format!("{}-proximity", device_id),
                device_id: device_id.clone(),
                point_id: "proximity_sensor".to_string(),
                pattern: PatternConfig::Random {
                    min: 0.0,
                    max: 1.0,
                    distribution: "uniform".to_string(),
                },
                interval_ms: options.interval_ms / 10,
            });

            // Temperature sensor
            points.push(ScenarioPoint {
                id: format!("{}-motor-temp", device_id),
                device_id: device_id.clone(),
                point_id: "motor_temperature".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 5.0,
                    offset: 45.0,
                    period_secs: 1800.0,
                    phase: 0.0,
                },
                interval_ms: options.interval_ms,
            });
        }

        // Production milestone event
        events.push(ScenarioEvent {
            name: "production-milestone".to_string(),
            trigger: EventTrigger::Periodic {
                interval_secs: 3600.0,
                start_secs: 3600.0,
            },
            actions: vec![EventAction::Log {
                message: "Production hour completed".to_string(),
                level: "info".to_string(),
            }],
        });

        Scenario {
            name: "Manufacturing Line Simulation".to_string(),
            description: format!("Simulates {} production stations", options.device_count),
            duration_secs: options.duration_secs,
            time_scale: options.time_scale,
            points,
            events,
            variables: options.variables,
        }
    })
}

/// Create energy monitoring template.
fn create_energy_template() -> Template {
    let info = TemplateInfo {
        id: "energy-monitoring".to_string(),
        name: "Energy Monitoring System".to_string(),
        description: "Simulates power meters, transformers, and energy management".to_string(),
        category: TemplateCategory::Energy,
        protocols: vec!["Modbus".to_string(), "BACnet".to_string()],
        device_count: 15,
        point_count: 75,
    };

    Template::new(info, |options| {
        let mut points = Vec::new();

        for i in 0..options.device_count {
            let device_id = format!("meter-{:03}", i + 1);
            let base_power = 10.0 + (i as f64) * 5.0; // Different base loads

            // Active power (kW)
            points.push(ScenarioPoint {
                id: format!("{}-power", device_id),
                device_id: device_id.clone(),
                point_id: "active_power".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: base_power * 0.3,
                    offset: base_power,
                    period_secs: 86400.0 / options.time_scale,
                    phase: 0.0,
                },
                interval_ms: options.interval_ms,
            });

            // Voltage
            points.push(ScenarioPoint {
                id: format!("{}-voltage", device_id),
                device_id: device_id.clone(),
                point_id: "voltage".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 480.0,
                    std_dev: 5.0,
                },
                interval_ms: options.interval_ms,
            });

            // Current
            points.push(ScenarioPoint {
                id: format!("{}-current", device_id),
                device_id: device_id.clone(),
                point_id: "current".to_string(),
                pattern: PatternConfig::Noise {
                    mean: base_power / 0.48, // Approximate current
                    std_dev: 2.0,
                },
                interval_ms: options.interval_ms,
            });

            // Power factor
            points.push(ScenarioPoint {
                id: format!("{}-pf", device_id),
                device_id: device_id.clone(),
                point_id: "power_factor".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 0.92,
                    std_dev: 0.02,
                },
                interval_ms: options.interval_ms,
            });

            // Energy (cumulative)
            points.push(ScenarioPoint {
                id: format!("{}-energy", device_id),
                device_id: device_id.clone(),
                point_id: "total_energy".to_string(),
                pattern: PatternConfig::Ramp {
                    start: 0.0,
                    end: base_power * 24.0, // Daily energy
                    duration_secs: 86400.0 / options.time_scale,
                    repeat: false,
                },
                interval_ms: options.interval_ms * 10,
            });
        }

        Scenario {
            name: "Energy Monitoring Simulation".to_string(),
            description: format!("Simulates {} power meters", options.device_count),
            duration_secs: options.duration_secs,
            time_scale: options.time_scale,
            points,
            events: vec![],
            variables: options.variables,
        }
    })
}

/// Create water treatment template.
fn create_water_template() -> Template {
    let info = TemplateInfo {
        id: "water-treatment".to_string(),
        name: "Water Treatment Plant".to_string(),
        description: "Simulates a water treatment facility with pumps, tanks, and analyzers".to_string(),
        category: TemplateCategory::Water,
        protocols: vec!["Modbus".to_string(), "BACnet".to_string()],
        device_count: 12,
        point_count: 60,
    };

    Template::new(info, |options| {
        let mut points = Vec::new();
        let mut events = Vec::new();

        // Tanks
        for i in 0..3.min(options.device_count / 4 + 1) {
            let device_id = format!("tank-{:03}", i + 1);

            points.push(ScenarioPoint {
                id: format!("{}-level", device_id),
                device_id: device_id.clone(),
                point_id: "level".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 20.0,
                    offset: 50.0, // 50% average
                    period_secs: 7200.0 / options.time_scale,
                    phase: (i as f64) * 2.0,
                },
                interval_ms: options.interval_ms,
            });

            points.push(ScenarioPoint {
                id: format!("{}-temperature", device_id),
                device_id: device_id.clone(),
                point_id: "temperature".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 18.0,
                    std_dev: 1.0,
                },
                interval_ms: options.interval_ms,
            });
        }

        // Pumps
        for i in 0..4.min(options.device_count / 3 + 1) {
            let device_id = format!("pump-{:03}", i + 1);

            points.push(ScenarioPoint {
                id: format!("{}-flow", device_id),
                device_id: device_id.clone(),
                point_id: "flow_rate".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![0.0, 50.0, 100.0, 75.0],
                    step_duration_secs: 1800.0,
                },
                interval_ms: options.interval_ms,
            });

            points.push(ScenarioPoint {
                id: format!("{}-pressure", device_id),
                device_id: device_id.clone(),
                point_id: "discharge_pressure".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 4.5, // bar
                    std_dev: 0.2,
                },
                interval_ms: options.interval_ms,
            });

            points.push(ScenarioPoint {
                id: format!("{}-status", device_id),
                device_id: device_id.clone(),
                point_id: "running_status".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![0.0, 1.0],
                    step_duration_secs: 3600.0,
                },
                interval_ms: options.interval_ms * 10,
            });
        }

        // Analyzers
        for i in 0..3.min(options.device_count / 4 + 1) {
            let device_id = format!("analyzer-{:03}", i + 1);

            points.push(ScenarioPoint {
                id: format!("{}-ph", device_id),
                device_id: device_id.clone(),
                point_id: "ph".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 7.2,
                    std_dev: 0.1,
                },
                interval_ms: options.interval_ms,
            });

            points.push(ScenarioPoint {
                id: format!("{}-chlorine", device_id),
                device_id: device_id.clone(),
                point_id: "chlorine".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 1.5, // ppm
                    std_dev: 0.2,
                },
                interval_ms: options.interval_ms,
            });

            points.push(ScenarioPoint {
                id: format!("{}-turbidity", device_id),
                device_id: device_id.clone(),
                point_id: "turbidity".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 0.5, // NTU
                    std_dev: 0.1,
                },
                interval_ms: options.interval_ms,
            });
        }

        // Low tank level alarm
        events.push(ScenarioEvent {
            name: "low-level-alarm".to_string(),
            trigger: EventTrigger::Condition {
                point: "tank-001-level".to_string(),
                operator: "<".to_string(),
                value: 20.0,
            },
            actions: vec![
                EventAction::Log {
                    message: "Tank level low - starting backup pump".to_string(),
                    level: "warn".to_string(),
                },
            ],
        });

        Scenario {
            name: "Water Treatment Simulation".to_string(),
            description: "Simulates water treatment plant operations".to_string(),
            duration_secs: options.duration_secs,
            time_scale: options.time_scale,
            points,
            events,
            variables: options.variables,
        }
    })
}

/// Create data center template.
fn create_datacenter_template() -> Template {
    let info = TemplateInfo {
        id: "datacenter-infrastructure".to_string(),
        name: "Data Center Infrastructure".to_string(),
        description: "Simulates data center cooling, power, and environmental monitoring".to_string(),
        category: TemplateCategory::DataCenter,
        protocols: vec!["Modbus".to_string(), "BACnet".to_string(), "OPC UA".to_string()],
        device_count: 25,
        point_count: 125,
    };

    Template::new(info, |options| {
        let mut points = Vec::new();

        // CRAC units
        for i in 0..5.min(options.device_count / 5 + 1) {
            let device_id = format!("crac-{:03}", i + 1);

            points.push(ScenarioPoint {
                id: format!("{}-supply-temp", device_id),
                device_id: device_id.clone(),
                point_id: "supply_air_temperature".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 16.0,
                    std_dev: 0.5,
                },
                interval_ms: options.interval_ms,
            });

            points.push(ScenarioPoint {
                id: format!("{}-return-temp", device_id),
                device_id: device_id.clone(),
                point_id: "return_air_temperature".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 28.0,
                    std_dev: 1.0,
                },
                interval_ms: options.interval_ms,
            });

            points.push(ScenarioPoint {
                id: format!("{}-humidity", device_id),
                device_id: device_id.clone(),
                point_id: "relative_humidity".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 45.0,
                    std_dev: 3.0,
                },
                interval_ms: options.interval_ms,
            });

            points.push(ScenarioPoint {
                id: format!("{}-power", device_id),
                device_id: device_id.clone(),
                point_id: "power_consumption".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 5.0,
                    offset: 25.0, // kW
                    period_secs: 3600.0 / options.time_scale,
                    phase: 0.0,
                },
                interval_ms: options.interval_ms,
            });
        }

        // PDUs
        for i in 0..10.min(options.device_count / 2) {
            let device_id = format!("pdu-{:03}", i + 1);
            let base_load = 15.0 + (i as f64) * 2.0;

            points.push(ScenarioPoint {
                id: format!("{}-power", device_id),
                device_id: device_id.clone(),
                point_id: "total_power".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: base_load * 0.2,
                    offset: base_load,
                    period_secs: 86400.0 / options.time_scale,
                    phase: 0.0,
                },
                interval_ms: options.interval_ms,
            });

            points.push(ScenarioPoint {
                id: format!("{}-voltage", device_id),
                device_id: device_id.clone(),
                point_id: "voltage".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 208.0,
                    std_dev: 2.0,
                },
                interval_ms: options.interval_ms,
            });
        }

        // Environmental sensors
        for i in 0..10.min(options.device_count / 2) {
            let device_id = format!("env-{:03}", i + 1);

            points.push(ScenarioPoint {
                id: format!("{}-temp", device_id),
                device_id: device_id.clone(),
                point_id: "temperature".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 22.0 + (i as f64) * 0.5,
                    std_dev: 1.0,
                },
                interval_ms: options.interval_ms,
            });

            points.push(ScenarioPoint {
                id: format!("{}-humidity", device_id),
                device_id: device_id.clone(),
                point_id: "humidity".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 45.0,
                    std_dev: 5.0,
                },
                interval_ms: options.interval_ms,
            });
        }

        Scenario {
            name: "Data Center Simulation".to_string(),
            description: "Simulates data center infrastructure monitoring".to_string(),
            duration_secs: options.duration_secs,
            time_scale: options.time_scale,
            points,
            events: vec![],
            variables: options.variables,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_registry() {
        let registry = TemplateRegistry::new();
        let templates = registry.list();

        assert!(!templates.is_empty());
        assert!(templates.iter().any(|t| t.id == "hvac-building"));
        assert!(templates.iter().any(|t| t.id == "manufacturing-line"));
    }

    #[test]
    fn test_hvac_template_generation() {
        let registry = TemplateRegistry::new();
        let options = TemplateOptions::default().with_devices(5);

        let scenario = registry.generate("hvac-building", options).unwrap();

        assert!(!scenario.name.is_empty());
        assert!(!scenario.points.is_empty());
        assert!(scenario.points.len() >= 25); // 5 devices * 5 points
    }

    #[test]
    fn test_manufacturing_template() {
        let registry = TemplateRegistry::new();
        let options = TemplateOptions::default().with_devices(3);

        let scenario = registry.generate("manufacturing-line", options).unwrap();

        assert!(!scenario.points.is_empty());
        // Check for expected point types
        assert!(scenario.points.iter().any(|p| p.point_id.contains("conveyor")));
    }

    #[test]
    fn test_category_filtering() {
        let registry = TemplateRegistry::new();

        let building_templates = registry.list_by_category(TemplateCategory::BuildingAutomation);
        assert!(building_templates.iter().any(|t| t.id == "hvac-building"));

        let manufacturing = registry.list_by_category(TemplateCategory::Manufacturing);
        assert!(manufacturing.iter().any(|t| t.id == "manufacturing-line"));
    }

    #[test]
    fn test_template_options() {
        let options = TemplateOptions::default()
            .with_devices(20)
            .with_time_scale(10.0)
            .with_duration(7200)
            .with_interval(500);

        assert_eq!(options.device_count, 20);
        assert!((options.time_scale - 10.0).abs() < f64::EPSILON);
        assert_eq!(options.duration_secs, 7200);
        assert_eq!(options.interval_ms, 500);
    }
}
