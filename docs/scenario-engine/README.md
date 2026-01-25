# Scenario Engine

The `mabi-scenario` crate provides a scenario engine for orchestrating time-based simulations across industrial protocol simulators. This document describes the architecture, components, and usage patterns of the scenario engine.

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Core Components](#core-components)
  - [Scenario Schema](#scenario-schema)
  - [Pattern Generator](#pattern-generator)
  - [Event System](#event-system)
  - [Scenario Player](#scenario-player)
  - [Scenario Executor](#scenario-executor)
- [Pattern Types](#pattern-types)
- [Event Triggers and Actions](#event-triggers-and-actions)
- [Validation](#validation)
- [YAML Schema Reference](#yaml-schema-reference)
- [API Reference](#api-reference)

---

## Overview

The scenario engine enables declarative definition of simulation behaviors through YAML configuration files. It supports:

- **Pattern-based value generation** for simulating sensor readings and device states
- **Time-based and condition-based event triggering** for dynamic scenario behavior
- **Time scaling** for accelerated or decelerated playback
- **Device integration** through an executor that bridges scenarios with protocol simulators

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         Scenario File                           │
│                     (YAML/JSON Definition)                      │
└─────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                       ScenarioParser                            │
│              (Deserialization & Format Detection)               │
└─────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                      ScenarioValidator                          │
│           (Schema Validation & Dependency Checking)             │
└─────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                       ScenarioPlayer                            │
│    ┌─────────────────┐    ┌─────────────────────────────┐      │
│    │ PatternGenerator│    │      EventManager           │      │
│    │  (per point)    │    │  (triggers & actions)       │      │
│    └─────────────────┘    └─────────────────────────────┘      │
│                                                                 │
│    ┌─────────────────┐    ┌─────────────────────────────┐      │
│    │  FollowManager  │    │      ReplayManager          │      │
│    │ (source tracking)│   │   (file-based playback)     │      │
│    └─────────────────┘    └─────────────────────────────┘      │
└─────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼ (ValueUpdate broadcast)
┌─────────────────────────────────────────────────────────────────┐
│                      ScenarioExecutor                           │
│    ┌─────────────────┐    ┌─────────────────────────────┐      │
│    │  DeviceRegistry │    │     ExecutorMetrics         │      │
│    │ (device handles)│    │   (write statistics)        │      │
│    └─────────────────┘    └─────────────────────────────┘      │
└─────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Protocol Simulators                         │
│        (Modbus, OPC UA, BACnet, KNX device instances)          │
└─────────────────────────────────────────────────────────────────┘
```

---

## Core Components

### Scenario Schema

The `Scenario` struct serves as the root container for scenario configuration:

| Field | Type | Description |
|-------|------|-------------|
| `name` | `String` | Scenario identifier (required) |
| `description` | `String` | Optional description |
| `duration_secs` | `u64` | Total duration; 0 indicates infinite |
| `time_scale` | `f64` | Playback speed multiplier (default: 1.0) |
| `points` | `Vec<ScenarioPoint>` | Data points to simulate |
| `events` | `Vec<ScenarioEvent>` | Event triggers and actions |
| `variables` | `HashMap<String, f64>` | Scenario-level variables |

Each `ScenarioPoint` defines:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `String` | Unique point identifier |
| `device_id` | `String` | Target device identifier |
| `point_id` | `String` | Point identifier within device |
| `pattern` | `PatternConfig` | Value generation pattern |
| `interval_ms` | `u64` | Update frequency in milliseconds (default: 1000) |

### Pattern Generator

The `PatternGenerator` produces values based on elapsed time and configured pattern parameters. Each generator maintains:

- Start time reference for elapsed time calculation
- Last generated value for stateful patterns
- Deterministic random number generator for reproducible sequences

Generation is performed via `generate()` for current time or `generate_at(time_secs)` for arbitrary timestamps.

### Event System

The event system comprises:

**EventManager**: Coordinates trigger evaluation and action execution. Maintains shared point values for condition evaluation and tracks event instance states.

**EventInstance**: Runtime state for each event including:
- Trigger state with edge detection for condition triggers
- Fire count tracking
- Enable/disable status

**EventBuilder**: Fluent API for programmatic event construction:

```rust
EventBuilder::new("temperature_alarm")
    .when("temperature", ">", 100.0)
    .set_value("alarm_flag", 1.0)
    .log("Temperature exceeded threshold", LogLevel::Warn)
    .build()
```

### Scenario Player

The `ScenarioPlayer` executes scenarios asynchronously with the following state machine:

```
Stopped → Running → Paused → Running → Completed
    ↓         ↓                    ↓
    └────────→ Stopped ←──────────┘
```

Configuration options (`PlayerConfig`):

| Field | Type | Description |
|-------|------|-------------|
| `time_scale` | `Option<f64>` | Override scenario time scale |
| `max_duration` | `Option<Duration>` | Maximum runtime limit |

The player broadcasts `ValueUpdate` messages through a Tokio broadcast channel, containing point identifier, generated value, and timestamp.

### Scenario Executor

The `ScenarioExecutor` bridges the player with protocol device instances:

| Configuration | Default | Description |
|---------------|---------|-------------|
| `write_timeout` | 5s | Per-device write deadline |
| `continue_on_error` | true | Continue on write failures |
| `max_concurrent_writes` | 100 | Concurrency limit |
| `collect_metrics` | true | Enable metrics collection |

The executor maintains a `DeviceRegistry` for device handle management and collects `ExecutorMetrics`:

- Values generated count
- Write attempts, successes, and failures
- Average write latency
- Per-device statistics

---

## Pattern Types

The engine supports nine pattern types:

### Constant

Static value output.

```yaml
pattern:
  type: constant
  value: 25.0
```

### Sine / Cosine

Sinusoidal waveform generation.

```yaml
pattern:
  type: sine
  amplitude: 5.0
  offset: 22.0
  period_secs: 3600
  phase: 0.0  # optional, default: 0.0
```

Formula: `offset + amplitude * sin(2π * t / period + phase)`

### Ramp

Linear interpolation between two values.

```yaml
pattern:
  type: ramp
  start: 0.0
  end: 100.0
  duration_secs: 600
  repeat: false  # optional, default: false
```

### Step

Cyclic progression through discrete levels.

```yaml
pattern:
  type: step
  levels: [0.0, 25.0, 50.0, 75.0, 100.0]
  step_duration_secs: 60
```

### Random

Random value generation within bounds.

```yaml
pattern:
  type: random
  min: 0.0
  max: 100.0
  distribution: uniform  # or "normal"/"gaussian"
```

For Gaussian distribution, standard deviation is calculated as `(max - min) / 6`.

### Noise

Gaussian noise generation.

```yaml
pattern:
  type: noise
  mean: 50.0
  std_dev: 5.0
```

### Follow

Derived value from another point with optional transformation and delay.

```yaml
pattern:
  type: follow
  source: "temperature_sensor"
  gain: 1.1       # optional, default: 1.0
  offset: 2.0     # optional, default: 0.0
  delay_ms: 500   # optional, default: 0
```

Formula: `source_value * gain + offset`

The delay implementation uses a circular buffer with linear interpolation between timestamped entries.

### Replay

Playback from recorded data files.

```yaml
pattern:
  type: replay
  file: "historical_data.csv"
  loop_replay: false  # optional, default: false
```

Supported formats:
- CSV with configurable column names (default: "time", "value")
- JSON with structure `{data: [{time: T, value: V}, ...]}`
- JSON Lines (one object per line)

---

## Event Triggers and Actions

### Trigger Types

**Time Trigger**: Single activation at specified time.

```yaml
trigger:
  type: time
  at_secs: 300.0
```

**Periodic Trigger**: Recurring activation at fixed intervals.

```yaml
trigger:
  type: periodic
  interval_secs: 60.0
  start_secs: 0.0  # optional, default: 0.0
```

**Condition Trigger**: Value-based activation with edge detection.

```yaml
trigger:
  type: condition
  point: "temperature"
  operator: ">="
  value: 50.0
```

Supported operators: `==`, `!=`, `<`, `<=`, `>`, `>=` (aliases: `eq`, `ne`, `lt`, `le`, `gt`, `ge`)

Condition triggers employ rising edge detection, firing only on false-to-true transitions.

### Action Types

**SetValue**: Assign a constant value to a point.

```yaml
- type: setvalue
  point: "alarm_flag"
  value: 1.0
```

**ChangePattern**: Modify a point's generation pattern.

```yaml
- type: changepattern
  point: "temperature"
  pattern:
    type: constant
    value: 25.0
```

**Log**: Output a message.

```yaml
- type: log
  message: "Event triggered"
  level: info  # debug, info, warn, error
```

**Pause**: Suspend scenario execution.

```yaml
- type: pause
```

**Stop**: Terminate scenario execution.

```yaml
- type: stop
```

---

## Validation

The `ScenarioValidator` performs comprehensive validation with configurable options:

| Option | Description |
|--------|-------------|
| `check_follow_sources` | Verify Follow pattern source points exist |
| `check_replay_files` | Verify replay files are accessible |
| `check_circular_deps` | Detect circular Follow dependencies |
| `warn_performance` | Warn on large point counts or frequent intervals |

Validation produces `ValidationIssue` items with severity levels (Info, Warning, Error) and specific issue codes for precise error identification.

---

## YAML Schema Reference

Complete scenario structure:

```yaml
name: "scenario_name"
description: "Optional description"
duration_secs: 3600
time_scale: 1.0

variables:
  setpoint: 25.0

points:
  - id: "point_id"
    device_id: "device_id"
    point_id: "point_within_device"
    interval_ms: 1000
    pattern:
      type: sine
      amplitude: 5.0
      offset: 22.0
      period_secs: 3600

events:
  - name: "event_name"
    trigger:
      type: condition
      point: "point_id"
      operator: ">="
      value: 30.0
    actions:
      - type: log
        message: "Condition met"
        level: warn
```

---

## API Reference

### Loading and Parsing

```rust
use mabi_scenario::prelude::*;

// Load from file (auto-detects format)
let scenario = ScenarioParser::load("scenario.yaml").await?;

// Parse from string
let scenario = ScenarioParser::parse_yaml(yaml_content)?;
```

### Validation

```rust
let validator = ScenarioValidator::new();
let result = validator.validate(&scenario);

if !result.is_valid() {
    for error in result.errors() {
        eprintln!("{}: {}", error.path, error.message);
    }
}
```

### Playback

```rust
let config = PlayerConfig::default();
let mut player = ScenarioPlayer::new(scenario, config);
let mut receiver = player.subscribe();

tokio::spawn(async move {
    player.run().await
});

while let Ok(update) = receiver.recv().await {
    println!("{}: {}", update.point_id, update.value);
}
```

### Execution with Devices

```rust
let config = ExecutorConfig::default();
let mut executor = ScenarioExecutor::new(scenario, config);

executor.register_device("device-001", device_handle);

let mut events = executor.subscribe();

executor.run().await?;
```

---

## Module Structure

```
crates/mabi-scenario/src/
├── lib.rs          # Module exports and error types
├── schema.rs       # Scenario, ScenarioPoint, PatternConfig, EventTrigger, EventAction
├── generator.rs    # PatternGenerator implementation
├── player.rs       # ScenarioPlayer, ValueUpdate, PlayerState
├── event.rs        # EventManager, EventInstance, EventBuilder
├── executor.rs     # ScenarioExecutor, DeviceRegistry, ExecutorMetrics
├── follow.rs       # FollowManager, DelayBuffer, SourceRegistry
├── replay.rs       # ReplayManager, ReplaySeries, ReplayLoader
├── validation.rs   # ScenarioValidator, ValidationResult
├── templates.rs    # Template registry and factories
└── parser.rs       # ScenarioParser for file I/O
```
