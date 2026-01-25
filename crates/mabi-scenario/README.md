# mabi-scenario

Scenario engine for the Mabinogion industrial protocol simulator.

## Overview

Time-based simulation orchestration engine for defining and executing complex test scenarios.

## Features

- YAML-based scenario definition
- Time-scaled execution (real-time, accelerated, or decelerated)
- Event scheduling and triggers
- Variable interpolation
- Conditional actions
- Looping and iteration

## Usage

```rust
use mabi_scenario::prelude::*;

// Load and run a scenario
let scenario = Scenario::from_file("test_scenario.yaml")?;

let engine = ScenarioEngine::new(scenario);
engine.run().await?;
```

## Scenario Format

```yaml
name: stress_test
duration: 10m
time_scale: 2.0

devices:
  - id: plc-001
    protocol: modbus_tcp

events:
  - at: 1m
    action: inject_latency
    params:
      delay: 100ms
```

## License

Licensed under the Apache License, Version 2.0.
