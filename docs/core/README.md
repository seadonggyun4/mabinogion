# mabi-core

Core abstractions, utilities, and orchestration for the Mabinogion industrial protocol simulator.

## Overview

The `mabi-core` crate provides the foundational infrastructure for building industrial protocol simulators. It defines common abstractions for devices, data points, protocols, and simulation engines that are shared across all protocol-specific crates.

## Modules

| Module | Description |
|--------|-------------|
| [`config`](#configuration) | Configuration management with hot reload, multi-format support, and validation |
| [`device`](#device-abstraction) | Core device trait, lifecycle states, and device handles |
| [`device_builder`](#device-builder) | Fluent builder API for device and data point configuration |
| [`engine`](#simulator-engine) | Central orchestrator for device management and simulation execution |
| [`error`](#error-handling) | Comprehensive error types with severity levels and validation |
| [`factory`](#factory-system) | Device factory pattern and plugin system for extensibility |
| [`lifecycle`](#lifecycle-management) | Device lifecycle state machine with recovery support |
| [`logging`](#logging) | Structured logging with tracing integration |
| [`metrics`](#metrics) | Prometheus-compatible metrics collection |
| [`protocol`](#protocol) | Protocol definitions (Modbus, OPC UA, BACnet, KNX) |
| [`tags`](#tags) | Unified device tagging system for organization and filtering |
| [`types`](#types) | Core data structures for data points, values, and addresses |
| [`capabilities`](#capabilities) | Protocol capability detection and feature querying |
| [`utils`](#utilities) | ID generation, retry logic, rate limiting, and time utilities |

## Tags

The `tags` module provides a unified tagging mechanism for organizing and filtering devices across all protocols. This system implements a multi-dimensional taxonomy based on established metadata classification principles from distributed systems literature.

### Theoretical Foundation

The tagging system addresses three fundamental challenges in industrial protocol simulation:

1. **Resource Identification**: Uniquely identifying simulated devices across heterogeneous protocol namespaces
2. **Organizational Taxonomy**: Hierarchical and cross-cutting classification for operational management
3. **Query Selectivity**: Efficient filtering and aggregation for metrics, logging, and operational queries

The design draws from label-based systems in container orchestration (Kubernetes labels/selectors), time-series databases (Prometheus label dimensions), and asset management frameworks (ISA-95 equipment hierarchy).

### Taxonomic Model

Tags support two orthogonal classification paradigms:

| Paradigm | Structure | Semantics | Use Case |
|----------|-----------|-----------|----------|
| **Key-Value Tags** | `key=value` pairs | Dimensional metadata with explicit attribute-value relationships | Hierarchical organization (location, floor, zone), numeric properties (unit_id, instance), environment classification (env=prod) |
| **Labels** | Set membership | Boolean predicates indicating group affiliation | Capability flags (critical, monitored), functional categories (hvac, lighting), operational states (maintenance) |

This dual-paradigm approach enables both **dimensional queries** (e.g., "all devices where location=building-a AND floor=3") and **categorical queries** (e.g., "all devices with label critical").

### Cardinality Considerations

Following Prometheus best practices for label design:

- **Low-cardinality tags** (location, protocol, environment): Suitable for metric aggregation dimensions
- **High-cardinality tags** (device_id, instance_number): Use with caution in metric labels; prefer for filtering only

The Tags structure imposes no cardinality limits, delegating policy enforcement to the consuming application (e.g., metrics exporters may filter high-cardinality labels).

### Tags Struct

```rust
use mabi_core::tags::Tags;

let tags = Tags::new()
    .with_tag("location", "building-a")
    .with_tag("floor", "3")
    .with_label("hvac")
    .with_label("critical");

assert!(tags.has_label("hvac"));
assert_eq!(tags.get("location"), Some("building-a"));
assert!(tags.matches_selector(&[("location", "building-a")]));
```

### Tags API

| Method | Description |
|--------|-------------|
| `new()` | Create empty tags |
| `from_map(HashMap)` | Create from existing HashMap |
| `from_pairs(iter)` | Create from key-value pairs |
| `with_tag(key, value)` | Add key-value tag (builder) |
| `with_label(label)` | Add label (builder) |
| `with_labels(iter)` | Add multiple labels (builder) |
| `with_tags(iter)` | Add multiple key-value tags (builder) |
| `insert(key, value)` | Add key-value tag (mutable) |
| `add_label(label)` | Add label (mutable) |
| `get(key)` | Get tag value by key |
| `has_label(label)` | Check if label exists |
| `contains_key(key)` | Check if tag key exists |
| `remove(key)` | Remove tag by key |
| `remove_label(label)` | Remove label |
| `merge(other)` | Merge another Tags into this |
| `merged_with(other)` | Create merged copy |
| `matches_selector(&[(&str, &str)])` | Check if all selector pairs match |
| `has_any_label(iter)` | Check if any specified labels exist |
| `has_all_labels(iter)` | Check if all specified labels exist |
| `is_empty()` | Check if tags are empty |
| `len()` | Total count of tags and labels |

### TagsBuilder

Fluent builder for constructing Tags:

```rust
use mabi_core::tags::TagsBuilder;

let tags = TagsBuilder::new()
    .tag("env", "prod")
    .tag("region", "us-west")
    .label("monitored")
    .build();
```

### Parsing Tag Strings

Parse tags from CLI-style strings (`key=value` or `label`):

```rust
use mabi_core::tags::{parse_tag_string, parse_tags};

// Single tag
let (key, value) = parse_tag_string("location=building-a")?;
assert_eq!(key, "location");
assert_eq!(value, Some("building-a".to_string()));

// Label (no value)
let (key, value) = parse_tag_string("critical")?;
assert_eq!(key, "critical");
assert_eq!(value, None);

// Multiple tags
let tags = parse_tags(&["location=building-a", "floor=3", "critical"])?;
assert_eq!(tags.get("location"), Some("building-a"));
assert!(tags.has_label("critical"));
```

### Taggable Trait

Extension trait for types that can have tags:

```rust
pub trait Taggable {
    fn tags(&self) -> &Tags;
    fn tags_mut(&mut self) -> &mut Tags;
    fn has_tag(&self, key: &str, value: &str) -> bool;
    fn has_label(&self, label: &str) -> bool;
}
```

### Serialization

Tags serialize to JSON/YAML with automatic empty field skipping:

```rust
let tags = Tags::new()
    .with_tag("location", "building-a")
    .with_label("critical");

// Serializes to:
// {"tags":{"location":"building-a"},"labels":["critical"]}

let empty_tags = Tags::new();
// Serializes to: {}
```

### Protocol Integration

Tags are supported across all four industrial protocols via the unified CLI interface:

| Protocol | CLI Command | Tag Application |
|----------|-------------|-----------------|
| Modbus TCP/RTU | `mabi serve modbus --tag key=value` | Applied to all unit IDs in the simulator |
| OPC UA | `mabi serve opcua --config <file> --session <name>` | Applied through canonical config/session metadata |
| BACnet/IP | `mabi serve bacnet --tag key=value` | Applied to device object metadata |
| KNXnet/IP | `mabi serve knx --tag key=value` | Applied to server-level metadata |

#### Cross-Protocol Tagging Example

```bash
# Deploy a unified building automation simulation with consistent tagging
mabi serve modbus --port 5020 --devices 10 --tag location=building-a --tag system=hvac &
mabi serve opcua --config opcua.yaml --session default &
mabi serve bacnet --port 47808 --objects 200 --tag location=building-a --tag system=bms &
mabi serve knx --port 3671 --groups 100 --tag location=building-a --tag system=lighting &
```

This enables unified monitoring and filtering:

```promql
# Prometheus query: aggregate all protocol metrics for building-a
sum(mabi_requests_total{location="building-a"}) by (protocol)
```

### Operational Patterns

#### 1. Environment Segregation

```bash
# Production environment
mabi serve modbus --tag env=prod --tag critical

# Development/testing
mabi serve modbus --tag env=dev --tag ephemeral
```

#### 2. ISA-95 Equipment Hierarchy

```bash
# Enterprise > Site > Area > Cell > Unit
mabi serve bacnet --tag enterprise=acme \
            --tag site=plant-01 \
            --tag area=packaging \
            --tag cell=line-3 \
            --tag unit=wrapper-01
```

#### 3. Functional Classification

```bash
# Cross-cutting functional categories
mabi serve opcua --config opcua.yaml --session default \
           --tag subsystem=chiller \
           --tag monitored \
           --tag critical
```

### Query Semantics

The `matches_selector` method implements conjunctive (AND) query semantics:

```rust
// Selector: all key-value pairs must match
tags.matches_selector(&[("location", "building-a"), ("floor", "3")])
// Returns true iff tags["location"] == "building-a" AND tags["floor"] == "3"
```

For disjunctive (OR) queries on labels:

```rust
// Any of the specified labels
tags.has_any_label(&["critical", "monitored"])

// All of the specified labels
tags.has_all_labels(&["critical", "monitored"])
```

Complex queries combining both paradigms:

```rust
// Devices in building-a that are either critical OR monitored
let matches = tags.matches_selector(&[("location", "building-a")])
    && tags.has_any_label(&["critical", "monitored"]);
```

---

## Device Abstraction

The `Device` trait defines the interface for all simulated devices:

```rust
#[async_trait]
pub trait Device: Send + Sync {
    fn info(&self) -> &DeviceInfo;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn protocol(&self) -> Protocol;
    fn state(&self) -> DeviceState;

    async fn initialize(&mut self) -> Result<()>;
    async fn start(&mut self) -> Result<()>;
    async fn stop(&mut self) -> Result<()>;
    async fn tick(&mut self) -> Result<()>;

    fn point_definitions(&self) -> Vec<&DataPointDef>;
    async fn read(&self, point_id: &str) -> Result<DataPoint>;
    async fn write(&mut self, point_id: &str, value: Value) -> Result<()>;

    fn statistics(&self) -> DeviceStatistics;
}
```

### Device States

Devices transition through the following states:

| State | Description |
|-------|-------------|
| `Uninitialized` | Device created but not yet initialized |
| `Initializing` | Initialization in progress |
| `Online` | Operational and accepting requests |
| `Offline` | Not operational |
| `Error` | Error state requiring intervention |
| `ShuttingDown` | Shutdown in progress |

### DeviceHandle

`DeviceHandle` provides thread-safe async access to devices with cached metadata:

```rust
let handle = engine.device("device-001")?;
let info = handle.info();  // Cached, no lock required
let point = handle.read("temperature").await?;
```

## Simulator Engine

The `SimulatorEngine` orchestrates all devices and manages the simulation lifecycle:

```rust
let engine = SimulatorEngineBuilder::new()
    .name("Industrial Simulator")
    .max_devices(10_000)
    .max_points(1_000_000)
    .tick_interval(Duration::from_millis(100))
    .build();

engine.start().await?;
engine.add_device(device).await?;

// Read/write operations
let point = engine.read_point("device-001", "temperature").await?;
engine.write_point("device-001", "setpoint", Value::F64(25.5)).await?;
```

### Engine Presets

Pre-configured settings for common use cases:

| Preset | Devices | Points | Tick Interval | Log Level |
|--------|---------|--------|---------------|-----------|
| `Development` | 100 | 10,000 | 500ms | debug |
| `Production` | 50,000 | 5,000,000 | 100ms | info |
| `Testing` | 10 | 100 | 10ms | trace |
| `StressTest` | 100,000 | 10,000,000 | 50ms | warn |

### Engine Events

Subscribe to engine events for monitoring:

```rust
let mut rx = engine.subscribe();
while let Ok(event) = rx.recv().await {
    match event {
        EngineEvent::DeviceAdded { device_id, protocol } => { /* ... */ }
        EngineEvent::DeviceStateChanged { device_id, old_state, new_state } => { /* ... */ }
        EngineEvent::Error { message } => { /* ... */ }
        _ => {}
    }
}
```

## Types

### Protocol

Supported industrial protocols:

```rust
pub enum Protocol {
    ModbusTcp,   // TCP/IP, default port 502
    ModbusRtu,   // Serial RTU
    OpcUa,       // TCP, default port 4840
    BacnetIp,    // UDP/IP, default port 47808
    KnxIp,       // UDP/IP, default port 3671
}
```

### Value

Dynamic value type supporting all industrial data types:

```rust
pub enum Value {
    Bool(bool),
    I8(i8), U8(u8), I16(i16), U16(u16),
    I32(i32), U32(u32), I64(i64), U64(u64),
    F32(f32), F64(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Null,
}
```

Conversion methods:
- `as_bool()`, `as_f64()`, `as_i64()`, `as_str()`, `as_bytes()`
- `to_registers()` / `from_registers()` for Modbus register conversion

### DataPoint

Represents a data point with value, quality, and timestamp:

```rust
pub struct DataPoint {
    pub id: DataPointId,        // device_id/point_id
    pub value: Value,
    pub quality: Quality,       // GOOD, BAD, UNCERTAIN with flags
    pub timestamp: DateTime<Utc>,
    pub units: Option<String>,
    pub description: Option<String>,
}
```

### Quality

Data quality flags following OPC UA quality model:

```rust
// Base qualities
Quality::GOOD
Quality::BAD
Quality::UNCERTAIN

// Additional flags
Quality::SUBSTITUTED
Quality::LOCAL_OVERRIDE
Quality::CONFIGURATION_ERROR
Quality::NOT_CONNECTED
Quality::DEVICE_FAILURE
```

### Address

Protocol-specific addressing:

```rust
pub enum Address {
    Modbus { register_type: RegisterType, address: u16, count: u16 },
    OpcUa { node_id: String },
    BacNet { object_type: ObjectType, instance: u32, property: PropertyId },
    Knx { group_address: String },
}
```

## Configuration

### EngineConfig

Primary configuration structure:

```rust
pub struct EngineConfig {
    pub name: String,
    pub max_devices: usize,         // Default: 10,000
    pub max_points: usize,          // Default: 1,000,000
    pub tick_interval_ms: u64,      // Default: 100
    pub workers: usize,             // Default: CPU count
    pub enable_metrics: bool,       // Default: true
    pub metrics_interval_secs: u64, // Default: 10
    pub log_level: String,          // Default: "info"
    pub protocols: HashMap<String, ProtocolConfig>,
}
```

### Multi-Format Support

Load configuration from YAML, JSON, or TOML:

```rust
let config: EngineConfig = ConfigLoader::load("config.yaml")?;
let config: EngineConfig = ConfigLoader::load("config.json")?;
let config: EngineConfig = ConfigLoader::load("config.toml")?;
```

### Environment Overrides

Environment variables override file configuration (prefix: `TRAP_SIM_`):

| Variable | Config Field |
|----------|--------------|
| `TRAP_SIM_ENGINE_NAME` | `name` |
| `TRAP_SIM_ENGINE_MAX_DEVICES` | `max_devices` |
| `TRAP_SIM_ENGINE_MAX_POINTS` | `max_points` |
| `TRAP_SIM_ENGINE_TICK_INTERVAL_MS` | `tick_interval_ms` |
| `TRAP_SIM_ENGINE_WORKERS` | `workers` |
| `TRAP_SIM_ENGINE_METRICS` | `enable_metrics` |
| `TRAP_SIM_LOG_LEVEL` | `log_level` |

### Hot Reload

Watch configuration files for changes:

```rust
let watcher = ConfigWatcher::new();
watcher.watch_file("config.yaml".into())?;

let mut rx = watcher.subscribe();
while let Ok(ConfigEvent::Changed { source, .. }) = rx.recv().await {
    // Reload configuration
}
```

## Error Handling

### Error Types

Comprehensive error enumeration:

```rust
pub enum Error {
    DeviceNotFound { device_id: String },
    DeviceAlreadyExists { device_id: String },
    DataPointNotFound { device_id: String, point_id: String },
    InvalidAddress { address: u32, min: u32, max: u32 },
    InvalidValue { point_id: String, reason: String },
    TypeMismatch { expected: String, actual: String },
    Validation { message: String, errors: ValidationErrors },
    Lifecycle { from: DeviceState, to: DeviceState },
    CapacityExceeded { current: usize, max: usize, resource: String },
    Timeout { duration_ms: u64 },
    AccessDenied { point_id: String, operation: String, mode: String },
    OutOfRange { point_id: String, value: f64, min: f64, max: f64 },
    // ... additional variants
}
```

### Error Severity

Errors are categorized by severity:

| Severity | Description |
|----------|-------------|
| `Low` | Validation errors, user input errors |
| `Medium` | Recoverable errors, timeouts |
| `High` | Protocol errors, state errors |
| `Critical` | Internal errors, engine failures |

### Validation Errors

Field-level validation error collection:

```rust
let mut errors = ValidationErrors::new();
errors.add("port", "must be between 1 and 65535");
errors.add("name", "cannot be empty");

if !errors.is_empty() {
    return Err(Error::Validation {
        message: "Configuration invalid".into(),
        errors,
    });
}
```

## Metrics

Prometheus-compatible metrics collection:

```rust
let metrics = MetricsCollector::global();

// Record operations
metrics.record_read("modbus", true, Duration::from_millis(5));
metrics.record_write("opcua", false, Duration::from_millis(100));
metrics.record_error("bacnet", "timeout");

// Time operations automatically
{
    let _timer = metrics.time_request("modbus", "read_holding_registers");
    // ... operation ...
} // Duration recorded on drop

// Update gauges
metrics.set_devices_active(1000);
metrics.set_connections_active("modbus", 50);

// Export
let snapshot = metrics.snapshot();
let prometheus_text = metrics.export_prometheus();
```

### Available Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `mabi_requests_total` | Counter | protocol, operation | Total requests |
| `mabi_reads_total` | Counter | protocol, status | Read operations |
| `mabi_writes_total` | Counter | protocol, status | Write operations |
| `mabi_errors_total` | Counter | protocol, error_type | Errors |
| `mabi_devices_active` | Gauge | - | Active devices |
| `mabi_data_points_total` | Gauge | - | Total data points |
| `mabi_request_duration_seconds` | Histogram | protocol, operation | Request latency |
| `mabi_tick_duration_seconds` | Histogram | - | Tick duration |

### Latency Statistics

Percentile-based latency analysis:

```rust
let stats = LatencyStats::from_samples(&durations);
println!("p50: {}us, p99: {}us", stats.p50_us, stats.p99_us);
```

## Factory System

### DeviceFactory

Trait for creating protocol-specific devices:

```rust
pub trait DeviceFactory: Send + Sync {
    fn protocol(&self) -> Protocol;
    fn create(&self, config: DeviceConfig) -> Result<BoxedDevice>;
    fn create_batch(&self, configs: Vec<DeviceConfig>) -> Result<Vec<BoxedDevice>>;
    fn validate(&self, config: &DeviceConfig) -> Result<()>;
    fn metadata(&self) -> FactoryMetadata;
}
```

### Factory Registry

Central registry for device factories:

```rust
let registry = FactoryRegistry::new();
registry.register(ModbusFactory)?;
registry.register(OpcUaFactory)?;

let device = registry.create_device(config)?;
```

### Plugin System

Runtime plugin loading:

```rust
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn description(&self) -> &str;
    fn initialize(&mut self) -> Result<()>;
    fn register_factories(&self, registry: &FactoryRegistry) -> Result<()>;
    fn shutdown(&mut self) -> Result<()>;
}

let manager = PluginManager::new(registry);
manager.load(MyPlugin)?;
```

## Lifecycle Management

### State Machine

`LifecycleStateMachine` manages valid state transitions:

```
Uninitialized -> Initializing -> Offline -> Online
      |              |            |          |
      v              v            v          v
    Error          Error        Error      Error
                                             |
                                             v
                                        ShuttingDown -> Offline
```

### Recovery

Automatic recovery with configurable retries:

```rust
let mut state_machine = LifecycleStateMachine::new()
    .with_retries(3, Duration::from_secs(5));

if state_machine.record_error() {
    // Retries exhausted, manual intervention required
}
```

### Lifecycle Hooks

Custom behavior at lifecycle transitions:

```rust
#[async_trait]
pub trait LifecycleHook: Send + Sync {
    async fn before_init(&self) -> Result<()> { Ok(()) }
    async fn after_init(&self) -> Result<()> { Ok(()) }
    async fn before_start(&self) -> Result<()> { Ok(()) }
    async fn after_start(&self) -> Result<()> { Ok(()) }
    async fn before_stop(&self) -> Result<()> { Ok(()) }
    async fn after_stop(&self) -> Result<()> { Ok(()) }
    async fn on_error(&self, error: &Error) -> Result<()> { Ok(()) }
}
```

## Capabilities

Query protocol capabilities at runtime:

```rust
let caps = default_capabilities(Protocol::OpcUa);

if caps.supports(Capability::Subscription) {
    // Enable subscriptions
}

if caps.supports_all(&[Capability::Read, Capability::Write, Capability::BatchRead]) {
    // Use batch operations
}
```

### Capability List

| Capability | Description |
|------------|-------------|
| `Read`, `Write` | Basic data operations |
| `BatchRead`, `BatchWrite` | Batch operations |
| `Subscription` | Change subscriptions |
| `ChangeOfValue` | COV notifications |
| `Deadband` | Deadband filtering |
| `HistoryRead`, `HistoryWrite` | Historical data |
| `Discovery`, `Browse` | Device/node discovery |
| `Authentication`, `Encryption` | Security features |
| `Alarms`, `Scheduling` | Advanced features |

### Default Capabilities by Protocol

| Protocol | Capabilities |
|----------|-------------|
| Modbus | Read, Write, BatchRead, BatchWrite, Browse |
| OPC UA | All core + Subscription, History, Security, Alarms |
| BACnet | Read, Write, BatchRead, COV, Discovery, TrendLog, Scheduling |
| KNX | Read, Write, Browse, Discovery, Group Communication |

## Device Builder

Fluent API for device configuration:

```rust
let config = DeviceConfigBuilder::new("sensor-001")
    .name("Temperature Sensor")
    .protocol(Protocol::ModbusTcp)
    .address("192.168.1.100:502")
    .metadata("location", "Building A")
    .tag("zone", "hvac")
    .tag("floor", "3")
    .label("monitored")
    .label("critical")
    .point(
        DataPointConfigBuilder::new("temperature")
            .name("Room Temperature")
            .data_type(DataType::Float32)
            .access(AccessMode::ReadOnly)
            .units("°C")
            .range(-40.0, 85.0)
            .address(Address::Modbus {
                register_type: RegisterType::Input,
                address: 0,
                count: 2,
            })
            .build()
    )
    .build();
```

### Device Builder Tag Methods

| Method | Description |
|--------|-------------|
| `tags(Tags)` | Set tags from existing Tags instance |
| `tag(key, value)` | Add a key-value tag |
| `label(label)` | Add a label |
| `labels(iter)` | Add multiple labels |

## Utilities

### ID Generation

```rust
let uuid = generate_uuid();           // "550e8400-e29b-41d4-a716-446655440000"
let short = generate_short_uuid();    // "550e8400"
let ts_id = generate_timestamp_id();  // "1706234567890-a1b2"
```

### Retry Logic

```rust
let config = RetryConfig {
    max_retries: 3,
    initial_delay: Duration::from_millis(100),
    backoff_factor: 2.0,
    max_delay: Duration::from_secs(10),
};

let result = retry_async(config, || async {
    // Operation that may fail
}).await?;
```

### Rate Limiting

```rust
let limiter = RateLimiter::new(100, Duration::from_secs(1));  // 100 req/sec

limiter.acquire().await;  // Blocks if rate exceeded
// Perform rate-limited operation
```

### Time Utilities

```rust
let ms = current_timestamp_ms();
let formatted = format_duration(Duration::from_secs(3661));  // "1h 1m 1s"
```

### String Utilities

```rust
let bytes = format_bytes(1_500_000);  // "1.43 MB"
let safe = sanitize_identifier("my-id@123");  // "my_id_123"
```

## Prelude

Import commonly used types with a single statement:

```rust
use mabi_core::prelude::*;
```

This includes:
- Error types: `Error`, `Result`, `ValidationErrors`
- Protocol types: `Protocol`, `Value`, `DataType`, `AccessMode`
- Data types: `DataPoint`, `DataPointDef`, `Quality`, `Address`
- Device types: `Device`, `DeviceInfo`, `DeviceState`, `DeviceStatistics`
- Tags: `Tags`, `TagsBuilder`, `Taggable`, `parse_tag_string`, `parse_tags`
- Configuration: `EngineConfig`, `DeviceConfig`, all protocol configs
- Engine: `SimulatorEngine`, `SimulatorEngineBuilder`, `EngineEvent`
- Factory: `DeviceFactory`, `FactoryRegistry`, `Plugin`, `PluginManager`
- Metrics: `MetricsCollector`, `MetricsSnapshot`, `Timer`
- Capabilities: `Capability`, `CapabilitySet`, `ProtocolCapabilities`
- Lifecycle: `DeviceLifecycle`, `LifecycleStateMachine`
- Utilities: ID generators, retry logic, rate limiter
- External: `async_trait`, tracing macros

## Testing

```bash
# Run all tests
cargo test --package mabi-core

# Run with output
cargo test --package mabi-core -- --nocapture

# Run specific module tests
cargo test --package mabi-core device::
cargo test --package mabi-core metrics::
```
