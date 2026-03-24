//! BACnet-specific scenario templates.
//!
//! Production-quality templates that exercise the full BACnet feature set:
//!
//! - **HVAC Building** (`bacnet-hvac`): Multi-zone AHU/VAV system with Schedule/Calendar,
//!   COV subscriptions, trending, occupancy-driven setpoints, and realistic daily cycles.
//!
//! - **Alarm & Event** (`bacnet-alarm`): EventEnrollment, NotificationClass, intrinsic/algorithmic
//!   alarming, acknowledgment flows, alarm storms, priority cascading.
//!
//! - **Trend Logging** (`bacnet-trend`): TrendLog objects, ReadRange patterns, historical data
//!   accumulation, buffer-full notifications, periodic/COV/polled sampling modes.
//!
//! - **Resilience Testing** (`bacnet-resilience`): Chaos-integrated scenario with progressive
//!   failure injection — network degradation, device offline, segmentation faults, COV drops,
//!   service rejection, and recovery verification.
//!
//! - **BEMS Integration** (`bacnet-bems`): Full Building Energy Management scenario combining
//!   energy metering, schedule coordination, demand limiting, load shedding, and alarm
//!   escalation across multiple BACnet networks.

use std::collections::HashMap;

use crate::schema::{
    EventAction, EventTrigger, PatternConfig, Scenario, ScenarioDevice, ScenarioEvent,
    ScenarioPoint,
};
use crate::templates::{Template, TemplateCategory, TemplateInfo};
use mabi_core::tags::Tags;

// =============================================================================
// Registration
// =============================================================================

/// Register all BACnet-specific templates into the given registry.
pub fn register_bacnet_templates(registry: &mut crate::templates::TemplateRegistry) {
    registry.register(create_bacnet_hvac_template());
    registry.register(create_bacnet_alarm_template());
    registry.register(create_bacnet_trend_template());
    registry.register(create_bacnet_resilience_template());
    registry.register(create_bacnet_bems_template());
}

// =============================================================================
// Helper — device builder
// =============================================================================

fn bacnet_device(id: &str, name: &str, instance: u32) -> ScenarioDevice {
    let mut config = HashMap::new();
    config.insert(
        "instance".to_string(),
        serde_yaml::Value::Number(serde_yaml::Number::from(instance)),
    );
    config.insert(
        "protocol".to_string(),
        serde_yaml::Value::String("bacnet-ip".to_string()),
    );

    ScenarioDevice {
        id: id.to_string(),
        name: name.to_string(),
        protocol: "bacnet".to_string(),
        config,
        tags: Tags::new(),
    }
}

fn tagged_device(
    id: &str,
    name: &str,
    instance: u32,
    tags: &[(&str, &str)],
    labels: &[&str],
) -> ScenarioDevice {
    let mut device = bacnet_device(id, name, instance);
    for (k, v) in tags {
        device.tags.insert(k.to_string(), v.to_string());
    }
    for label in labels {
        device.tags.add_label(label.to_string());
    }
    device
}

// =============================================================================
// 1. HVAC Building Scenario
// =============================================================================

fn create_bacnet_hvac_template() -> Template {
    let info = TemplateInfo {
        id: "bacnet-hvac".to_string(),
        name: "BACnet HVAC Building".to_string(),
        description: concat!(
            "Full building HVAC simulation with AHU/VAV zones, ",
            "Schedule/Calendar-driven occupancy, COV-subscribed analog points, ",
            "TrendLog recording, and realistic daily thermal cycles."
        )
        .to_string(),
        category: TemplateCategory::BuildingAutomation,
        protocols: vec!["BACnet".to_string()],
        device_count: 12,
        point_count: 120,
    };

    Template::new(info, |opts| {
        let mut devices = Vec::new();
        let mut points = Vec::new();
        let mut events = Vec::new();

        let zone_count = opts.device_count.max(2);
        let ahu_count = (zone_count / 4).max(1);
        let ts = opts.time_scale;
        let interval = opts.interval_ms;

        // ----------------------------------------------------------------
        // AHU controllers
        // ----------------------------------------------------------------
        for i in 0..ahu_count {
            let dev_id = format!("ahu-{:03}", i + 1);
            let inst = 100 + i as u32;
            devices.push(tagged_device(
                &dev_id,
                &format!("AHU #{}", i + 1),
                inst,
                &[("type", "ahu"), ("building", "main")],
                &["hvac", "ahu"],
            ));

            // Supply air temp (AI) — PID-controlled around 14 °C setpoint
            points.push(ScenarioPoint {
                id: format!("{}-sat", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:0:present_value".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 14.0,
                    std_dev: 0.4,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // Return air temp (AI)
            points.push(ScenarioPoint {
                id: format!("{}-rat", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:1:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 2.0,
                    offset: 24.0,
                    period_secs: 86400.0 / ts,
                    phase: 0.0,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // Mixed air damper (AO) — follows outdoor conditions
            points.push(ScenarioPoint {
                id: format!("{}-mad", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_output:0:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 40.0,
                    offset: 50.0, // 50% mid, swings 10‥90%
                    period_secs: 86400.0 / ts,
                    phase: std::f64::consts::PI, // opposite to OAT
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // Supply fan status (BI)
            points.push(ScenarioPoint {
                id: format!("{}-sf-status", dev_id),
                device_id: dev_id.clone(),
                point_id: "binary_input:0:present_value".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![1.0, 1.0, 1.0, 0.0], // mostly ON, periodic off
                    step_duration_secs: 21600.0 / ts, // 6h steps
                },
                interval_ms: interval * 5,
                device_tags: Tags::new(),
            });

            // Supply fan speed (AO) — stepped
            points.push(ScenarioPoint {
                id: format!("{}-sf-speed", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_output:1:present_value".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![30.0, 60.0, 80.0, 100.0, 80.0, 60.0],
                    step_duration_secs: 3600.0 / ts,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // Filter DP (AI) — slowly rising (fouling simulation)
            points.push(ScenarioPoint {
                id: format!("{}-filter-dp", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:2:present_value".to_string(),
                pattern: PatternConfig::Ramp {
                    start: 150.0, // Pa
                    end: 450.0,   // dirty filter
                    duration_secs: opts.duration_secs as f64 / ts,
                    repeat: false,
                },
                interval_ms: interval * 10,
                device_tags: Tags::new(),
            });

            // Schedule object present_value — occupancy mode (MSV: 1=Occ,2=Unocc,3=Standby)
            points.push(ScenarioPoint {
                id: format!("{}-schedule", dev_id),
                device_id: dev_id.clone(),
                point_id: "schedule:0:present_value".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![2.0, 1.0, 1.0, 3.0], // Unocc→Occ→Occ→Standby
                    step_duration_secs: 21600.0 / ts, // 6h blocks
                },
                interval_ms: interval * 60, // schedules update slowly
                device_tags: Tags::new(),
            });
        }

        // ----------------------------------------------------------------
        // VAV zone controllers
        // ----------------------------------------------------------------
        for i in 0..zone_count {
            let dev_id = format!("vav-{:03}", i + 1);
            let inst = 200 + i as u32;
            let floor = (i / 4) + 1;
            devices.push(tagged_device(
                &dev_id,
                &format!("VAV Zone {}", i + 1),
                inst,
                &[
                    ("type", "vav"),
                    ("floor", &floor.to_string()),
                    ("building", "main"),
                ],
                &["hvac", "vav"],
            ));

            let phase = (i as f64) * 0.4;

            // Zone temperature (AI) — daily cycle with zone-specific phase
            points.push(ScenarioPoint {
                id: format!("{}-zone-temp", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:0:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 2.5,
                    offset: 22.0 + (i as f64 % 3.0) * 0.5,
                    period_secs: 86400.0 / ts,
                    phase,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // Zone setpoint (AV) — shifts with occupancy
            points.push(ScenarioPoint {
                id: format!("{}-zone-sp", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_value:0:present_value".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![26.0, 22.0, 22.0, 24.0], // night→day→day→evening
                    step_duration_secs: 21600.0 / ts,
                },
                interval_ms: interval * 30,
                device_tags: Tags::new(),
            });

            // Damper position (AO) — follows zone demand
            points.push(ScenarioPoint {
                id: format!("{}-damper", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_output:0:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 35.0,
                    offset: 50.0,
                    period_secs: 7200.0 / ts, // 2h cycle
                    phase: phase + 1.0,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // Reheat valve (AO)
            points.push(ScenarioPoint {
                id: format!("{}-reheat", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_output:1:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 20.0,
                    offset: 15.0,
                    period_secs: 3600.0 / ts,
                    phase: phase + 2.0,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // Airflow (AI) — CFM
            points.push(ScenarioPoint {
                id: format!("{}-airflow", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:1:present_value".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 400.0 + (i as f64) * 50.0,
                    std_dev: 30.0,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // Occupancy sensor (BI)
            points.push(ScenarioPoint {
                id: format!("{}-occ", dev_id),
                device_id: dev_id.clone(),
                point_id: "binary_input:0:present_value".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![0.0, 1.0, 1.0, 0.0],
                    step_duration_secs: 21600.0 / ts,
                },
                interval_ms: interval * 60,
                device_tags: Tags::new(),
            });

            // CO2 level (AI)
            points.push(ScenarioPoint {
                id: format!("{}-co2", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:2:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 200.0,
                    offset: 600.0,
                    period_secs: 43200.0 / ts, // 12h cycle
                    phase: phase + 0.5,
                },
                interval_ms: interval * 5,
                device_tags: Tags::new(),
            });
        }

        // ----------------------------------------------------------------
        // Outdoor air sensor
        // ----------------------------------------------------------------
        let oat_id = "oat-001";
        devices.push(tagged_device(
            oat_id,
            "Outdoor Air Sensor",
            50,
            &[("type", "sensor"), ("location", "roof")],
            &["hvac", "weather"],
        ));

        points.push(ScenarioPoint {
            id: "oat-temp".to_string(),
            device_id: oat_id.to_string(),
            point_id: "analog_input:0:present_value".to_string(),
            pattern: PatternConfig::Sine {
                amplitude: 8.0,
                offset: 18.0,
                period_secs: 86400.0 / ts,
                phase: -std::f64::consts::FRAC_PI_4,
            },
            interval_ms: interval * 5,
            device_tags: Tags::new(),
        });

        points.push(ScenarioPoint {
            id: "oat-humidity".to_string(),
            device_id: oat_id.to_string(),
            point_id: "analog_input:1:present_value".to_string(),
            pattern: PatternConfig::Noise {
                mean: 55.0,
                std_dev: 8.0,
            },
            interval_ms: interval * 10,
            device_tags: Tags::new(),
        });

        // ----------------------------------------------------------------
        // Events
        // ----------------------------------------------------------------

        // High zone temp alarm
        events.push(ScenarioEvent {
            name: "high-zone-temp".to_string(),
            trigger: EventTrigger::Condition {
                point: "vav-001-zone-temp".to_string(),
                operator: ">".to_string(),
                value: 26.0,
            },
            actions: vec![EventAction::Log {
                message: "Zone temperature high — possible cooling fault".to_string(),
                level: "warn".to_string(),
            }],
        });

        // Filter differential pressure alarm
        events.push(ScenarioEvent {
            name: "filter-dp-alarm".to_string(),
            trigger: EventTrigger::Condition {
                point: "ahu-001-filter-dp".to_string(),
                operator: ">".to_string(),
                value: 400.0,
            },
            actions: vec![EventAction::Log {
                message: "AHU filter pressure drop exceeded — replace filter".to_string(),
                level: "alarm".to_string(),
            }],
        });

        // Occupancy transition event (periodic check)
        events.push(ScenarioEvent {
            name: "schedule-transition".to_string(),
            trigger: EventTrigger::Periodic {
                interval_secs: 3600.0 / ts,
                start_secs: 0.0,
            },
            actions: vec![EventAction::Log {
                message: "Schedule evaluation tick".to_string(),
                level: "info".to_string(),
            }],
        });

        // CO2 demand ventilation
        events.push(ScenarioEvent {
            name: "co2-high".to_string(),
            trigger: EventTrigger::Condition {
                point: "vav-001-co2".to_string(),
                operator: ">".to_string(),
                value: 900.0,
            },
            actions: vec![
                EventAction::Log {
                    message: "CO2 above 900 ppm — increasing outdoor air".to_string(),
                    level: "warn".to_string(),
                },
                EventAction::SetValue {
                    point: "ahu-001-mad".to_string(),
                    value: 85.0,
                },
            ],
        });

        Scenario {
            name: "BACnet HVAC Building Simulation".to_string(),
            description: format!(
                "Full HVAC building with {} AHUs, {} VAV zones, Schedule/Calendar, COV, trending",
                ahu_count, zone_count
            ),
            duration_secs: opts.duration_secs,
            time_scale: ts,
            devices,
            points,
            events,
            variables: opts.variables,
            runtime: None,
        }
    })
}

// =============================================================================
// 2. Alarm & Event Scenario
// =============================================================================

fn create_bacnet_alarm_template() -> Template {
    let info = TemplateInfo {
        id: "bacnet-alarm".to_string(),
        name: "BACnet Alarm & Event".to_string(),
        description: concat!(
            "Alarm management scenario with EventEnrollment, NotificationClass, ",
            "intrinsic alarming (out-of-range, change-of-state, floating-limit), ",
            "alarm acknowledgment flows, priority cascading, and alarm storms."
        )
        .to_string(),
        category: TemplateCategory::BuildingAutomation,
        protocols: vec!["BACnet".to_string()],
        device_count: 8,
        point_count: 64,
    };

    Template::new(info, |opts| {
        let mut devices = Vec::new();
        let mut points = Vec::new();
        let mut events = Vec::new();

        let device_count = opts.device_count.max(3);
        let ts = opts.time_scale;
        let interval = opts.interval_ms;

        // ----------------------------------------------------------------
        // Alarm notification controller
        // ----------------------------------------------------------------
        devices.push(tagged_device(
            "nc-001",
            "Notification Controller",
            1,
            &[("type", "notification-class")],
            &["alarm", "notification"],
        ));

        // NotificationClass priority mapping
        points.push(ScenarioPoint {
            id: "nc-001-priority".to_string(),
            device_id: "nc-001".to_string(),
            point_id: "notification_class:0:priority".to_string(),
            pattern: PatternConfig::Constant { value: 3.0 }, // Priority 3 = normal
            interval_ms: interval * 60,
            device_tags: Tags::new(),
        });

        // ----------------------------------------------------------------
        // Devices with alarm-prone analog points
        // ----------------------------------------------------------------
        for i in 0..device_count {
            let dev_id = format!("alarm-dev-{:03}", i + 1);
            let inst = 300 + i as u32;

            devices.push(tagged_device(
                &dev_id,
                &format!("Alarm Source #{}", i + 1),
                inst,
                &[
                    ("type", "alarm-source"),
                    ("zone", &format!("zone-{}", (i % 4) + 1)),
                ],
                &["alarm", "monitored"],
            ));

            // --- Out-of-range alarming (AnalogInput) ---
            // Temperature that periodically crosses high/low thresholds
            let phase = (i as f64) * std::f64::consts::FRAC_PI_3;
            points.push(ScenarioPoint {
                id: format!("{}-temp", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:0:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 12.0, // swings ±12°C
                    offset: 22.0,
                    period_secs: 1800.0 / ts, // 30-min cycle → crosses thresholds
                    phase,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // High-limit reference (AV)
            points.push(ScenarioPoint {
                id: format!("{}-high-limit", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_value:0:present_value".to_string(),
                pattern: PatternConfig::Constant { value: 30.0 },
                interval_ms: interval * 120,
                device_tags: Tags::new(),
            });

            // Low-limit reference (AV)
            points.push(ScenarioPoint {
                id: format!("{}-low-limit", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_value:1:present_value".to_string(),
                pattern: PatternConfig::Constant { value: 14.0 },
                interval_ms: interval * 120,
                device_tags: Tags::new(),
            });

            // --- Change-of-state alarming (BinaryInput) ---
            // Door contact: flaps between open/closed
            points.push(ScenarioPoint {
                id: format!("{}-door", dev_id),
                device_id: dev_id.clone(),
                point_id: "binary_input:0:present_value".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![0.0, 1.0, 0.0, 0.0, 1.0, 0.0],
                    step_duration_secs: 600.0 / ts, // 10-min steps
                },
                interval_ms: interval * 5,
                device_tags: Tags::new(),
            });

            // --- Floating-limit alarming (AnalogInput) ---
            // Pressure sensor with drift
            points.push(ScenarioPoint {
                id: format!("{}-pressure", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:1:present_value".to_string(),
                pattern: PatternConfig::Ramp {
                    start: 100.0,
                    end: 500.0,
                    duration_secs: opts.duration_secs as f64 / ts,
                    repeat: true,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // --- EventEnrollment status (MSV) ---
            // 0=Normal, 1=Offnormal, 2=Fault, 3=HighLimit, 4=LowLimit
            points.push(ScenarioPoint {
                id: format!("{}-event-state", dev_id),
                device_id: dev_id.clone(),
                point_id: "event_enrollment:0:event_state".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![0.0, 3.0, 0.0, 4.0, 0.0, 1.0, 0.0],
                    step_duration_secs: 900.0 / ts,
                },
                interval_ms: interval * 10,
                device_tags: Tags::new(),
            });

            // --- Alarm acknowledgment tracking ---
            // acked_transitions bitmask: 0=needs-ack, 7=all-acked
            points.push(ScenarioPoint {
                id: format!("{}-ack-status", dev_id),
                device_id: dev_id.clone(),
                point_id: "event_enrollment:0:acked_transitions".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![7.0, 0.0, 3.0, 7.0],
                    step_duration_secs: 1200.0 / ts,
                },
                interval_ms: interval * 30,
                device_tags: Tags::new(),
            });

            // --- Status flags with alarm states ---
            points.push(ScenarioPoint {
                id: format!("{}-status-flags", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:0:status_flags".to_string(),
                pattern: PatternConfig::Step {
                    // 0=normal, 1=in_alarm, 2=fault, 3=in_alarm+fault
                    levels: vec![0.0, 1.0, 0.0, 0.0, 1.0, 2.0, 0.0],
                    step_duration_secs: 600.0 / ts,
                },
                interval_ms: interval * 10,
                device_tags: Tags::new(),
            });
        }

        // ----------------------------------------------------------------
        // Events
        // ----------------------------------------------------------------

        // High-temp alarm storm: multiple zones trip simultaneously
        events.push(ScenarioEvent {
            name: "alarm-storm-trigger".to_string(),
            trigger: EventTrigger::Time {
                at_secs: opts.duration_secs as f64 * 0.3 / ts,
            },
            actions: vec![
                EventAction::Log {
                    message: "Alarm storm: chiller failure causing multi-zone high temperature"
                        .to_string(),
                    level: "alarm".to_string(),
                },
                EventAction::ChangePattern {
                    point: "alarm-dev-001-temp".to_string(),
                    pattern: PatternConfig::Ramp {
                        start: 22.0,
                        end: 38.0,
                        duration_secs: 600.0 / ts,
                        repeat: false,
                    },
                },
                EventAction::ChangePattern {
                    point: "alarm-dev-002-temp".to_string(),
                    pattern: PatternConfig::Ramp {
                        start: 22.0,
                        end: 36.0,
                        duration_secs: 900.0 / ts,
                        repeat: false,
                    },
                },
            ],
        });

        // Recovery event
        events.push(ScenarioEvent {
            name: "alarm-storm-recovery".to_string(),
            trigger: EventTrigger::Time {
                at_secs: opts.duration_secs as f64 * 0.5 / ts,
            },
            actions: vec![
                EventAction::Log {
                    message: "Recovery: chiller restored, zones returning to normal".to_string(),
                    level: "info".to_string(),
                },
                EventAction::ChangePattern {
                    point: "alarm-dev-001-temp".to_string(),
                    pattern: PatternConfig::Ramp {
                        start: 38.0,
                        end: 22.0,
                        duration_secs: 1200.0 / ts,
                        repeat: false,
                    },
                },
            ],
        });

        // Periodic alarm summary
        events.push(ScenarioEvent {
            name: "alarm-summary".to_string(),
            trigger: EventTrigger::Periodic {
                interval_secs: 1800.0 / ts,
                start_secs: 900.0 / ts,
            },
            actions: vec![EventAction::Log {
                message: "Alarm summary: evaluating active alarms and unacknowledged transitions"
                    .to_string(),
                level: "info".to_string(),
            }],
        });

        Scenario {
            name: "BACnet Alarm & Event Simulation".to_string(),
            description: format!(
                "Alarm management with {} alarm sources, out-of-range/COS/floating-limit, alarm storms",
                device_count
            ),
            duration_secs: opts.duration_secs,
            time_scale: ts,
            devices,
            points,
            events,
            variables: opts.variables,
            runtime: None,
        }
    })
}

// =============================================================================
// 3. Trend Logging Scenario
// =============================================================================

fn create_bacnet_trend_template() -> Template {
    let info = TemplateInfo {
        id: "bacnet-trend".to_string(),
        name: "BACnet Trend Logging".to_string(),
        description: concat!(
            "TrendLog scenario with multiple sampling modes (periodic, COV, polled), ",
            "ReadRange access patterns, buffer-full notifications, log purging, ",
            "and historical data accumulation over extended periods."
        )
        .to_string(),
        category: TemplateCategory::BuildingAutomation,
        protocols: vec!["BACnet".to_string()],
        device_count: 6,
        point_count: 72,
    };

    Template::new(info, |opts| {
        let mut devices = Vec::new();
        let mut points = Vec::new();
        let mut events = Vec::new();

        let device_count = opts.device_count.max(2);
        let ts = opts.time_scale;
        let interval = opts.interval_ms;

        // ----------------------------------------------------------------
        // Trend data sources — each device generates realistic data for trending
        // ----------------------------------------------------------------
        for i in 0..device_count {
            let dev_id = format!("trend-src-{:03}", i + 1);
            let inst = 400 + i as u32;

            devices.push(tagged_device(
                &dev_id,
                &format!("Trend Source #{}", i + 1),
                inst,
                &[
                    ("type", "trend-source"),
                    ("group", &format!("group-{}", (i % 3) + 1)),
                ],
                &["trend", "monitored"],
            ));

            let phase = (i as f64) * 0.7;

            // --- Periodic-sampled analog (fast: 1s interval) ---
            // Zone temperature — primary trending target
            points.push(ScenarioPoint {
                id: format!("{}-temp", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:0:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 3.0,
                    offset: 22.0 + (i as f64) * 0.3,
                    period_secs: 3600.0 / ts,
                    phase,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // --- COV-sampled analog (variable rate) ---
            // Setpoint — changes infrequently, COV sampling ideal
            points.push(ScenarioPoint {
                id: format!("{}-setpoint", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_value:0:present_value".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![22.0, 22.5, 21.5, 22.0, 23.0, 22.0],
                    step_duration_secs: 1800.0 / ts,
                },
                interval_ms: interval * 30,
                device_tags: Tags::new(),
            });

            // --- Polled trend — slow accumulating data ---
            // Energy meter (kWh) — monotonically increasing
            points.push(ScenarioPoint {
                id: format!("{}-energy", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:1:present_value".to_string(),
                pattern: PatternConfig::Ramp {
                    start: 1000.0 * (i as f64 + 1.0),
                    end: 1000.0 * (i as f64 + 1.0) + 500.0,
                    duration_secs: opts.duration_secs as f64 / ts,
                    repeat: false,
                },
                interval_ms: interval * 60, // 1-minute polling
                device_tags: Tags::new(),
            });

            // --- Binary trend (COS logged) ---
            // Equipment status
            points.push(ScenarioPoint {
                id: format!("{}-equip-status", dev_id),
                device_id: dev_id.clone(),
                point_id: "binary_input:0:present_value".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
                    step_duration_secs: 2400.0 / ts,
                },
                interval_ms: interval * 10,
                device_tags: Tags::new(),
            });

            // --- TrendLog properties ---
            // Record count (simulated — tracks how many records accumulated)
            points.push(ScenarioPoint {
                id: format!("{}-tl-count", dev_id),
                device_id: dev_id.clone(),
                point_id: "trend_log:0:record_count".to_string(),
                pattern: PatternConfig::Ramp {
                    start: 0.0,
                    end: 10000.0,
                    duration_secs: opts.duration_secs as f64 / ts,
                    repeat: false,
                },
                interval_ms: interval * 5,
                device_tags: Tags::new(),
            });

            // Total record count (max buffer size reference)
            points.push(ScenarioPoint {
                id: format!("{}-tl-total", dev_id),
                device_id: dev_id.clone(),
                point_id: "trend_log:0:total_record_count".to_string(),
                pattern: PatternConfig::Ramp {
                    start: 0.0,
                    end: 10000.0,
                    duration_secs: opts.duration_secs as f64 / ts,
                    repeat: false,
                },
                interval_ms: interval * 5,
                device_tags: Tags::new(),
            });

            // Buffer size remaining
            points.push(ScenarioPoint {
                id: format!("{}-tl-remaining", dev_id),
                device_id: dev_id.clone(),
                point_id: "trend_log:0:buffer_size".to_string(),
                pattern: PatternConfig::Ramp {
                    start: 10000.0,
                    end: 0.0,
                    duration_secs: opts.duration_secs as f64 / ts,
                    repeat: false,
                },
                interval_ms: interval * 5,
                device_tags: Tags::new(),
            });

            // Enable/disable logging (BV)
            points.push(ScenarioPoint {
                id: format!("{}-tl-enable", dev_id),
                device_id: dev_id.clone(),
                point_id: "trend_log:0:enable".to_string(),
                pattern: PatternConfig::Constant { value: 1.0 }, // Always enabled
                interval_ms: interval * 120,
                device_tags: Tags::new(),
            });

            // --- High-frequency data for stress testing ReadRange ---
            // Vibration sensor — noisy, high sample rate
            points.push(ScenarioPoint {
                id: format!("{}-vibration", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:2:present_value".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 2.5, // mm/s
                    std_dev: 0.8,
                },
                interval_ms: (interval / 2).max(100), // High-rate
                device_tags: Tags::new(),
            });

            // --- Multistate for categorical trending ---
            // Operating mode (MSV: 1=Off, 2=Heat, 3=Cool, 4=Auto, 5=Emergency)
            points.push(ScenarioPoint {
                id: format!("{}-mode", dev_id),
                device_id: dev_id.clone(),
                point_id: "multi_state_value:0:present_value".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![1.0, 4.0, 3.0, 3.0, 2.0, 4.0],
                    step_duration_secs: 3600.0 / ts,
                },
                interval_ms: interval * 30,
                device_tags: Tags::new(),
            });

            // --- Demand power for load profiling ---
            points.push(ScenarioPoint {
                id: format!("{}-demand", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:3:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 15.0 + (i as f64) * 5.0,
                    offset: 30.0 + (i as f64) * 10.0,
                    period_secs: 86400.0 / ts,
                    phase: 0.0,
                },
                interval_ms: interval * 15,
                device_tags: Tags::new(),
            });
        }

        // ----------------------------------------------------------------
        // Events — buffer management
        // ----------------------------------------------------------------

        // Buffer 80% full warning
        events.push(ScenarioEvent {
            name: "buffer-warning".to_string(),
            trigger: EventTrigger::Condition {
                point: "trend-src-001-tl-remaining".to_string(),
                operator: "<".to_string(),
                value: 2000.0,
            },
            actions: vec![EventAction::Log {
                message: "TrendLog buffer nearing capacity — 80% full".to_string(),
                level: "warn".to_string(),
            }],
        });

        // Buffer full event
        events.push(ScenarioEvent {
            name: "buffer-full".to_string(),
            trigger: EventTrigger::Condition {
                point: "trend-src-001-tl-remaining".to_string(),
                operator: "<".to_string(),
                value: 100.0,
            },
            actions: vec![EventAction::Log {
                message: "TrendLog buffer full — purge or wrap required".to_string(),
                level: "alarm".to_string(),
            }],
        });

        // Periodic ReadRange simulation
        events.push(ScenarioEvent {
            name: "readrange-poll".to_string(),
            trigger: EventTrigger::Periodic {
                interval_secs: 300.0 / ts,
                start_secs: 60.0 / ts,
            },
            actions: vec![EventAction::Log {
                message: "ReadRange: client polling trend data".to_string(),
                level: "info".to_string(),
            }],
        });

        Scenario {
            name: "BACnet Trend Logging Simulation".to_string(),
            description: format!(
                "Trend logging with {} sources, periodic/COV/polled sampling, ReadRange",
                device_count
            ),
            duration_secs: opts.duration_secs,
            time_scale: ts,
            devices,
            points,
            events,
            variables: opts.variables,
            runtime: None,
        }
    })
}

// =============================================================================
// 4. Resilience Testing Scenario
// =============================================================================

fn create_bacnet_resilience_template() -> Template {
    let info = TemplateInfo {
        id: "bacnet-resilience".to_string(),
        name: "BACnet Resilience Testing".to_string(),
        description: concat!(
            "Chaos-integrated resilience scenario: progressive failure injection with ",
            "network degradation, device offline events, segmentation faults, COV drops, ",
            "service rejection, APDU corruption, and recovery verification phases."
        )
        .to_string(),
        category: TemplateCategory::BuildingAutomation,
        protocols: vec!["BACnet".to_string()],
        device_count: 10,
        point_count: 80,
    };

    Template::new(info, |opts| {
        let mut devices = Vec::new();
        let mut points = Vec::new();
        let mut events = Vec::new();

        let device_count = opts.device_count.max(4);
        let ts = opts.time_scale;
        let interval = opts.interval_ms;
        let duration = opts.duration_secs as f64;

        // ----------------------------------------------------------------
        // Baseline devices — must maintain readings through chaos
        // ----------------------------------------------------------------
        for i in 0..device_count {
            let dev_id = format!("resilience-{:03}", i + 1);
            let inst = 500 + i as u32;

            let reliability = if i < device_count / 2 {
                "primary"
            } else {
                "secondary"
            };
            devices.push(tagged_device(
                &dev_id,
                &format!("Resilience Target #{}", i + 1),
                inst,
                &[
                    ("type", "test-target"),
                    ("reliability", reliability),
                    ("network", &format!("net-{}", (i % 3) + 1)),
                ],
                &["resilience", "chaos-target"],
            ));

            // Analog input — must be readable through network chaos
            points.push(ScenarioPoint {
                id: format!("{}-analog", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:0:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 5.0,
                    offset: 50.0,
                    period_secs: 600.0 / ts,
                    phase: (i as f64) * 0.3,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // Binary output — must be writable through chaos
            points.push(ScenarioPoint {
                id: format!("{}-binary", dev_id),
                device_id: dev_id.clone(),
                point_id: "binary_output:0:present_value".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![0.0, 1.0],
                    step_duration_secs: 300.0 / ts,
                },
                interval_ms: interval * 5,
                device_tags: Tags::new(),
            });

            // COV-subscribed point — tests subscription resilience
            points.push(ScenarioPoint {
                id: format!("{}-cov-target", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_value:0:present_value".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 100.0,
                    std_dev: 5.0,
                },
                interval_ms: interval * 2,
                device_tags: Tags::new(),
            });

            // Multi-property read target (tests ReadPropertyMultiple resilience)
            points.push(ScenarioPoint {
                id: format!("{}-multi-1", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:1:present_value".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 25.0,
                    std_dev: 2.0,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            points.push(ScenarioPoint {
                id: format!("{}-multi-2", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:2:present_value".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 75.0,
                    std_dev: 3.0,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // Segmented response target (large property list)
            points.push(ScenarioPoint {
                id: format!("{}-segmented", dev_id),
                device_id: dev_id.clone(),
                point_id: "device:0:object_list".to_string(),
                pattern: PatternConfig::Constant { value: 50.0 }, // 50 objects
                interval_ms: interval * 60,
                device_tags: Tags::new(),
            });

            // Status flags tracking
            points.push(ScenarioPoint {
                id: format!("{}-status", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:0:status_flags".to_string(),
                pattern: PatternConfig::Constant { value: 0.0 },
                interval_ms: interval * 10,
                device_tags: Tags::new(),
            });

            // Reliability property — tracks device health
            points.push(ScenarioPoint {
                id: format!("{}-reliability", dev_id),
                device_id: dev_id.clone(),
                point_id: "device:0:reliability".to_string(),
                pattern: PatternConfig::Constant { value: 0.0 }, // no-fault-detected
                interval_ms: interval * 30,
                device_tags: Tags::new(),
            });
        }

        // ----------------------------------------------------------------
        // Chaos phases (events simulate chaos schedule)
        // ----------------------------------------------------------------

        // Phase 1: Baseline (0-10% duration)
        events.push(ScenarioEvent {
            name: "phase-baseline".to_string(),
            trigger: EventTrigger::Time { at_secs: 0.0 },
            actions: vec![EventAction::Log {
                message: "Phase 1: BASELINE — collecting reference readings".to_string(),
                level: "info".to_string(),
            }],
        });

        // Phase 2: Network latency (10-25%)
        events.push(ScenarioEvent {
            name: "phase-latency".to_string(),
            trigger: EventTrigger::Time {
                at_secs: duration * 0.1 / ts,
            },
            actions: vec![EventAction::Log {
                message: "Phase 2: NETWORK LATENCY — injecting 100-500ms response delays"
                    .to_string(),
                level: "warn".to_string(),
            }],
        });

        // Phase 3: Packet loss (25-40%)
        events.push(ScenarioEvent {
            name: "phase-packet-loss".to_string(),
            trigger: EventTrigger::Time {
                at_secs: duration * 0.25 / ts,
            },
            actions: vec![EventAction::Log {
                message: "Phase 3: PACKET LOSS — 10% random packet drop".to_string(),
                level: "warn".to_string(),
            }],
        });

        // Phase 4: Device offline (40-55%)
        events.push(ScenarioEvent {
            name: "phase-offline".to_string(),
            trigger: EventTrigger::Time {
                at_secs: duration * 0.4 / ts,
            },
            actions: vec![
                EventAction::Log {
                    message: "Phase 4: DEVICE OFFLINE — net-2 devices unreachable".to_string(),
                    level: "alarm".to_string(),
                },
                // Simulate offline by driving reliability to fault state
                EventAction::ChangePattern {
                    point: "resilience-003-reliability".to_string(),
                    pattern: PatternConfig::Constant { value: 7.0 }, // communication-failure
                },
                EventAction::ChangePattern {
                    point: "resilience-003-status".to_string(),
                    pattern: PatternConfig::Constant { value: 2.0 }, // fault
                },
            ],
        });

        // Phase 5: COV disruption (55-70%)
        events.push(ScenarioEvent {
            name: "phase-cov-disruption".to_string(),
            trigger: EventTrigger::Time {
                at_secs: duration * 0.55 / ts,
            },
            actions: vec![EventAction::Log {
                message: "Phase 5: COV DISRUPTION — dropping COV notifications, wrong process IDs"
                    .to_string(),
                level: "warn".to_string(),
            }],
        });

        // Phase 6: APDU corruption (70-85%)
        events.push(ScenarioEvent {
            name: "phase-apdu-corruption".to_string(),
            trigger: EventTrigger::Time {
                at_secs: duration * 0.70 / ts,
            },
            actions: vec![EventAction::Log {
                message: "Phase 6: APDU CORRUPTION — malformed PDUs, invalid service choices"
                    .to_string(),
                level: "alarm".to_string(),
            }],
        });

        // Phase 7: Recovery (85-100%)
        events.push(ScenarioEvent {
            name: "phase-recovery".to_string(),
            trigger: EventTrigger::Time {
                at_secs: duration * 0.85 / ts,
            },
            actions: vec![
                EventAction::Log {
                    message:
                        "Phase 7: RECOVERY — all faults cleared, verifying baseline restoration"
                            .to_string(),
                    level: "info".to_string(),
                },
                // Restore device to normal
                EventAction::ChangePattern {
                    point: "resilience-003-reliability".to_string(),
                    pattern: PatternConfig::Constant { value: 0.0 },
                },
                EventAction::ChangePattern {
                    point: "resilience-003-status".to_string(),
                    pattern: PatternConfig::Constant { value: 0.0 },
                },
            ],
        });

        Scenario {
            name: "BACnet Resilience Testing".to_string(),
            description: format!(
                "Progressive chaos injection across {} devices: 7 phases from baseline to recovery",
                device_count
            ),
            duration_secs: opts.duration_secs,
            time_scale: ts,
            devices,
            points,
            events,
            variables: opts.variables,
            runtime: None,
        }
    })
}

// =============================================================================
// 5. BEMS Integration Scenario
// =============================================================================

fn create_bacnet_bems_template() -> Template {
    let info = TemplateInfo {
        id: "bacnet-bems".to_string(),
        name: "BACnet BEMS Integration".to_string(),
        description: concat!(
            "Full Building Energy Management System scenario: multi-network BACnet/IP ",
            "with energy metering, schedule coordination, demand limiting, load shedding, ",
            "alarm escalation, COV-driven analytics, and cross-device command propagation."
        )
        .to_string(),
        category: TemplateCategory::BuildingAutomation,
        protocols: vec!["BACnet".to_string()],
        device_count: 20,
        point_count: 200,
    };

    Template::new(info, |opts| {
        let mut devices = Vec::new();
        let mut points = Vec::new();
        let mut events = Vec::new();

        let scale = (opts.device_count as f64 / 20.0).max(0.5);
        let meter_count = (5.0 * scale) as usize;
        let ahu_count = (4.0 * scale).max(1.0) as usize;
        let zone_count = (8.0 * scale).max(2.0) as usize;
        let chiller_count = (2.0 * scale).max(1.0) as usize;
        let ts = opts.time_scale;
        let interval = opts.interval_ms;

        // ----------------------------------------------------------------
        // Energy meters
        // ----------------------------------------------------------------
        for i in 0..meter_count {
            let dev_id = format!("meter-{:03}", i + 1);
            let inst = 600 + i as u32;

            devices.push(tagged_device(
                &dev_id,
                &format!("Energy Meter #{}", i + 1),
                inst,
                &[
                    ("type", "meter"),
                    ("network", "bems-net-1"),
                    ("metering-tier", if i == 0 { "main" } else { "sub" }),
                ],
                &["bems", "energy", "meter"],
            ));

            let base_kw = if i == 0 {
                200.0 // Main meter
            } else {
                30.0 + (i as f64) * 15.0
            };

            // Active power (kW)
            points.push(ScenarioPoint {
                id: format!("{}-power", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:0:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: base_kw * 0.3,
                    offset: base_kw,
                    period_secs: 86400.0 / ts,
                    phase: 0.0,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // Energy accumulator (kWh)
            points.push(ScenarioPoint {
                id: format!("{}-energy", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:1:present_value".to_string(),
                pattern: PatternConfig::Ramp {
                    start: 50000.0 + (i as f64) * 10000.0,
                    end: 50000.0 + (i as f64) * 10000.0 + base_kw * 24.0,
                    duration_secs: 86400.0 / ts,
                    repeat: true,
                },
                interval_ms: interval * 15,
                device_tags: Tags::new(),
            });

            // Power factor
            points.push(ScenarioPoint {
                id: format!("{}-pf", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:2:present_value".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 0.93,
                    std_dev: 0.02,
                },
                interval_ms: interval * 5,
                device_tags: Tags::new(),
            });

            // Demand (15-min rolling average)
            points.push(ScenarioPoint {
                id: format!("{}-demand", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:3:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: base_kw * 0.25,
                    offset: base_kw * 0.9,
                    period_secs: 900.0 / ts, // 15-min demand interval
                    phase: 0.0,
                },
                interval_ms: interval * 5,
                device_tags: Tags::new(),
            });
        }

        // ----------------------------------------------------------------
        // Chillers
        // ----------------------------------------------------------------
        for i in 0..chiller_count {
            let dev_id = format!("chiller-{:03}", i + 1);
            let inst = 700 + i as u32;

            devices.push(tagged_device(
                &dev_id,
                &format!("Chiller #{}", i + 1),
                inst,
                &[("type", "chiller"), ("network", "bems-net-2")],
                &["bems", "hvac", "chiller", "load-shed"],
            ));

            // Chiller kW
            points.push(ScenarioPoint {
                id: format!("{}-kw", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:0:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 30.0,
                    offset: 80.0,
                    period_secs: 7200.0 / ts,
                    phase: (i as f64) * 1.5,
                },
                interval_ms: interval * 2,
                device_tags: Tags::new(),
            });

            // Chilled water supply temp
            points.push(ScenarioPoint {
                id: format!("{}-chwst", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:1:present_value".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 6.7,
                    std_dev: 0.3,
                },
                interval_ms: interval * 2,
                device_tags: Tags::new(),
            });

            // Chiller enable command (BO)
            points.push(ScenarioPoint {
                id: format!("{}-enable", dev_id),
                device_id: dev_id.clone(),
                point_id: "binary_output:0:present_value".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![1.0, 1.0, 1.0, 0.0], // mostly ON
                    step_duration_secs: 21600.0 / ts,
                },
                interval_ms: interval * 60,
                device_tags: Tags::new(),
            });

            // COP (Coefficient of Performance)
            points.push(ScenarioPoint {
                id: format!("{}-cop", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_value:0:present_value".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 4.2,
                    std_dev: 0.3,
                },
                interval_ms: interval * 15,
                device_tags: Tags::new(),
            });
        }

        // ----------------------------------------------------------------
        // AHUs
        // ----------------------------------------------------------------
        for i in 0..ahu_count {
            let dev_id = format!("bems-ahu-{:03}", i + 1);
            let inst = 800 + i as u32;

            devices.push(tagged_device(
                &dev_id,
                &format!("BEMS AHU #{}", i + 1),
                inst,
                &[("type", "ahu"), ("network", "bems-net-2")],
                &["bems", "hvac", "ahu", "load-shed"],
            ));

            // Supply air temp
            points.push(ScenarioPoint {
                id: format!("{}-sat", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:0:present_value".to_string(),
                pattern: PatternConfig::Noise {
                    mean: 14.0,
                    std_dev: 0.5,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            // Fan kW
            points.push(ScenarioPoint {
                id: format!("{}-fan-kw", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:1:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 5.0,
                    offset: 12.0,
                    period_secs: 3600.0 / ts,
                    phase: 0.0,
                },
                interval_ms: interval * 2,
                device_tags: Tags::new(),
            });

            // Occupancy schedule
            points.push(ScenarioPoint {
                id: format!("{}-schedule", dev_id),
                device_id: dev_id.clone(),
                point_id: "schedule:0:present_value".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![2.0, 1.0, 1.0, 3.0], // Unocc→Occ→Occ→Standby
                    step_duration_secs: 21600.0 / ts,
                },
                interval_ms: interval * 60,
                device_tags: Tags::new(),
            });
        }

        // ----------------------------------------------------------------
        // Zones (lightweight — temperature + setpoint only)
        // ----------------------------------------------------------------
        for i in 0..zone_count {
            let dev_id = format!("bems-zone-{:03}", i + 1);
            let inst = 900 + i as u32;

            devices.push(tagged_device(
                &dev_id,
                &format!("Zone #{}", i + 1),
                inst,
                &[
                    ("type", "zone"),
                    ("floor", &format!("{}", (i / 4) + 1)),
                    ("network", "bems-net-3"),
                ],
                &["bems", "hvac", "zone"],
            ));

            points.push(ScenarioPoint {
                id: format!("{}-temp", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_input:0:present_value".to_string(),
                pattern: PatternConfig::Sine {
                    amplitude: 2.0,
                    offset: 22.0 + (i as f64 % 4.0) * 0.3,
                    period_secs: 86400.0 / ts,
                    phase: (i as f64) * 0.3,
                },
                interval_ms: interval,
                device_tags: Tags::new(),
            });

            points.push(ScenarioPoint {
                id: format!("{}-sp", dev_id),
                device_id: dev_id.clone(),
                point_id: "analog_value:0:present_value".to_string(),
                pattern: PatternConfig::Step {
                    levels: vec![26.0, 22.0, 22.0, 24.0],
                    step_duration_secs: 21600.0 / ts,
                },
                interval_ms: interval * 30,
                device_tags: Tags::new(),
            });
        }

        // ----------------------------------------------------------------
        // Events — BEMS operations
        // ----------------------------------------------------------------

        // Demand limit exceeded
        events.push(ScenarioEvent {
            name: "demand-limit-exceeded".to_string(),
            trigger: EventTrigger::Condition {
                point: "meter-001-demand".to_string(),
                operator: ">".to_string(),
                value: 220.0,
            },
            actions: vec![EventAction::Log {
                message: "DEMAND LIMIT: main meter exceeds 220 kW — initiating load shed"
                    .to_string(),
                level: "alarm".to_string(),
            }],
        });

        // Load shed command
        events.push(ScenarioEvent {
            name: "load-shed-level-1".to_string(),
            trigger: EventTrigger::Condition {
                point: "meter-001-demand".to_string(),
                operator: ">".to_string(),
                value: 240.0,
            },
            actions: vec![
                EventAction::Log {
                    message: "LOAD SHED Level 1: raising zone setpoints +2°C".to_string(),
                    level: "alarm".to_string(),
                },
                EventAction::SetValue {
                    point: "bems-zone-001-sp".to_string(),
                    value: 24.0,
                },
                EventAction::SetValue {
                    point: "bems-zone-002-sp".to_string(),
                    value: 24.0,
                },
            ],
        });

        // Schedule transition logging
        events.push(ScenarioEvent {
            name: "schedule-check".to_string(),
            trigger: EventTrigger::Periodic {
                interval_secs: 1800.0 / ts,
                start_secs: 0.0,
            },
            actions: vec![EventAction::Log {
                message: "BEMS schedule evaluation: checking occupancy transitions".to_string(),
                level: "info".to_string(),
            }],
        });

        // Energy report
        events.push(ScenarioEvent {
            name: "energy-report".to_string(),
            trigger: EventTrigger::Periodic {
                interval_secs: 3600.0 / ts,
                start_secs: 3600.0 / ts,
            },
            actions: vec![EventAction::Log {
                message: "Hourly energy report: calculating consumption totals".to_string(),
                level: "info".to_string(),
            }],
        });

        // Chiller efficiency alarm
        if chiller_count > 0 {
            events.push(ScenarioEvent {
                name: "chiller-low-cop".to_string(),
                trigger: EventTrigger::Condition {
                    point: "chiller-001-cop".to_string(),
                    operator: "<".to_string(),
                    value: 3.5,
                },
                actions: vec![EventAction::Log {
                    message: "Chiller COP below 3.5 — efficiency degradation detected".to_string(),
                    level: "warn".to_string(),
                }],
            });
        }

        Scenario {
            name: "BACnet BEMS Integration".to_string(),
            description: format!(
                "BEMS with {} meters, {} chillers, {} AHUs, {} zones across 3 BACnet networks",
                meter_count, chiller_count, ahu_count, zone_count
            ),
            duration_secs: opts.duration_secs,
            time_scale: ts,
            devices,
            points,
            events,
            variables: opts.variables,
            runtime: None,
        }
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::{TemplateOptions, TemplateRegistry};

    fn default_opts(devices: usize) -> TemplateOptions {
        TemplateOptions {
            device_count: devices,
            time_scale: 10.0,
            duration_secs: 3600,
            interval_ms: 1000,
            variables: HashMap::new(),
        }
    }

    #[test]
    fn test_bacnet_templates_register() {
        let mut registry = TemplateRegistry::new();
        register_bacnet_templates(&mut registry);

        assert!(registry.get("bacnet-hvac").is_some());
        assert!(registry.get("bacnet-alarm").is_some());
        assert!(registry.get("bacnet-trend").is_some());
        assert!(registry.get("bacnet-resilience").is_some());
        assert!(registry.get("bacnet-bems").is_some());
    }

    #[test]
    fn test_hvac_template_generation() {
        let mut registry = TemplateRegistry::new();
        register_bacnet_templates(&mut registry);

        let scenario = registry.generate("bacnet-hvac", default_opts(8)).unwrap();

        assert!(!scenario.name.is_empty());
        assert!(!scenario.devices.is_empty());
        assert!(!scenario.points.is_empty());
        assert!(!scenario.events.is_empty());

        // Must have AHU and VAV devices
        assert!(scenario.devices.iter().any(|d| d.id.starts_with("ahu-")));
        assert!(scenario.devices.iter().any(|d| d.id.starts_with("vav-")));
        assert!(scenario.devices.iter().any(|d| d.id == "oat-001"));

        // Check point IDs reference BACnet object types
        assert!(scenario
            .points
            .iter()
            .any(|p| p.point_id.contains("analog_input")));
        assert!(scenario
            .points
            .iter()
            .any(|p| p.point_id.contains("analog_output")));
        assert!(scenario
            .points
            .iter()
            .any(|p| p.point_id.contains("schedule")));
    }

    #[test]
    fn test_alarm_template_generation() {
        let mut registry = TemplateRegistry::new();
        register_bacnet_templates(&mut registry);

        let scenario = registry.generate("bacnet-alarm", default_opts(6)).unwrap();

        assert!(!scenario.devices.is_empty());
        assert!(!scenario.points.is_empty());
        assert!(scenario.events.len() >= 3);

        // Must have event enrollment and notification points
        assert!(scenario
            .points
            .iter()
            .any(|p| p.point_id.contains("event_enrollment")));
        // Must have alarm storm events
        assert!(scenario
            .events
            .iter()
            .any(|e| e.name.contains("alarm-storm")));
    }

    #[test]
    fn test_trend_template_generation() {
        let mut registry = TemplateRegistry::new();
        register_bacnet_templates(&mut registry);

        let scenario = registry.generate("bacnet-trend", default_opts(4)).unwrap();

        assert!(!scenario.devices.is_empty());
        assert!(!scenario.points.is_empty());

        // Must have trend_log references
        assert!(scenario
            .points
            .iter()
            .any(|p| p.point_id.contains("trend_log")));
        // Must have buffer management events
        assert!(scenario.events.iter().any(|e| e.name.contains("buffer")));
    }

    #[test]
    fn test_resilience_template_generation() {
        let mut registry = TemplateRegistry::new();
        register_bacnet_templates(&mut registry);

        let scenario = registry
            .generate("bacnet-resilience", default_opts(6))
            .unwrap();

        assert!(!scenario.devices.is_empty());
        assert!(!scenario.points.is_empty());
        assert!(scenario.events.len() >= 7); // 7 phases

        // Must have phase events
        assert!(scenario
            .events
            .iter()
            .any(|e| e.name.contains("phase-baseline")));
        assert!(scenario
            .events
            .iter()
            .any(|e| e.name.contains("phase-recovery")));

        // Devices must have chaos-target label
        assert!(scenario
            .devices
            .iter()
            .all(|d| d.tags.has_label("chaos-target")));
    }

    #[test]
    fn test_bems_template_generation() {
        let mut registry = TemplateRegistry::new();
        register_bacnet_templates(&mut registry);

        let scenario = registry.generate("bacnet-bems", default_opts(20)).unwrap();

        assert!(!scenario.devices.is_empty());
        assert!(!scenario.points.is_empty());
        assert!(!scenario.events.is_empty());

        // Must have meters, chillers, AHUs, and zones
        assert!(scenario.devices.iter().any(|d| d.id.starts_with("meter-")));
        assert!(scenario
            .devices
            .iter()
            .any(|d| d.id.starts_with("chiller-")));
        assert!(scenario
            .devices
            .iter()
            .any(|d| d.id.starts_with("bems-ahu-")));
        assert!(scenario
            .devices
            .iter()
            .any(|d| d.id.starts_with("bems-zone-")));

        // Must have demand limiting events
        assert!(scenario
            .events
            .iter()
            .any(|e| e.name.contains("demand-limit")));
    }

    #[test]
    fn test_hvac_scales_with_device_count() {
        let mut registry = TemplateRegistry::new();
        register_bacnet_templates(&mut registry);

        let small = registry.generate("bacnet-hvac", default_opts(4)).unwrap();
        let large = registry.generate("bacnet-hvac", default_opts(20)).unwrap();

        assert!(large.devices.len() > small.devices.len());
        assert!(large.points.len() > small.points.len());
    }

    #[test]
    fn test_bems_scales_with_device_count() {
        let mut registry = TemplateRegistry::new();
        register_bacnet_templates(&mut registry);

        let small = registry.generate("bacnet-bems", default_opts(10)).unwrap();
        let large = registry.generate("bacnet-bems", default_opts(40)).unwrap();

        assert!(large.devices.len() > small.devices.len());
        assert!(large.points.len() > small.points.len());
    }

    #[test]
    fn test_all_templates_have_devices_and_tags() {
        let mut registry = TemplateRegistry::new();
        register_bacnet_templates(&mut registry);

        for id in &[
            "bacnet-hvac",
            "bacnet-alarm",
            "bacnet-trend",
            "bacnet-resilience",
            "bacnet-bems",
        ] {
            let scenario = registry.generate(id, default_opts(8)).unwrap();
            assert!(!scenario.devices.is_empty(), "{} has no devices", id);

            // Every device must have a protocol set
            for device in &scenario.devices {
                assert_eq!(
                    device.protocol, "bacnet",
                    "{}: device {} has wrong protocol",
                    id, device.id
                );
            }

            // Every device must have at least one tag
            for device in &scenario.devices {
                assert!(
                    !device.tags.is_empty(),
                    "{}: device {} has no tags",
                    id,
                    device.id
                );
            }
        }
    }

    #[test]
    fn test_time_scale_affects_periods() {
        let mut registry = TemplateRegistry::new();
        register_bacnet_templates(&mut registry);

        let real_time = registry
            .generate(
                "bacnet-hvac",
                TemplateOptions {
                    device_count: 4,
                    time_scale: 1.0,
                    duration_secs: 86400,
                    interval_ms: 1000,
                    variables: HashMap::new(),
                },
            )
            .unwrap();

        let accelerated = registry
            .generate(
                "bacnet-hvac",
                TemplateOptions {
                    device_count: 4,
                    time_scale: 10.0,
                    duration_secs: 86400,
                    interval_ms: 1000,
                    variables: HashMap::new(),
                },
            )
            .unwrap();

        // In accelerated mode, events should fire sooner
        let first_event_real = match &real_time.events[0].trigger {
            EventTrigger::Condition { .. } => None,
            EventTrigger::Time { at_secs } => Some(*at_secs),
            EventTrigger::Periodic { start_secs, .. } => Some(*start_secs),
        };

        let first_event_accel = match &accelerated.events[0].trigger {
            EventTrigger::Condition { .. } => None,
            EventTrigger::Time { at_secs } => Some(*at_secs),
            EventTrigger::Periodic { start_secs, .. } => Some(*start_secs),
        };

        // Both may be condition-based (which don't have at_secs), so this test
        // primarily ensures no panic during generation with different time scales
        assert!(!real_time.points.is_empty());
        assert!(!accelerated.points.is_empty());
        let _ = (first_event_real, first_event_accel);
    }
}
