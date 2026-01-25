# mabi-core

Core abstractions and utilities for the Mabinogion industrial protocol simulator.

## Overview

This crate provides the foundational types, traits, and utilities used across all Mabinogion protocol simulators.

## Features

- Protocol-agnostic device and data point abstractions
- Metrics collection and export (Prometheus)
- Structured logging with tracing
- Configuration management with hot-reload support
- Common error types and result handling

## Usage

```rust
use mabi_core::prelude::*;

// Create a data point
let point = DataPoint::new("temperature", Value::Float(23.5));

// Initialize logging
let config = LogConfig::development();
init_logging(&config)?;
```

## License

Licensed under the Apache License, Version 2.0.
