# Chaos Engine

The `mabi-chaos` crate provides a comprehensive chaos engineering framework for injecting controlled faults into industrial protocol simulations. This module enables systematic testing of client resilience through configurable fault injection, time-based scheduling, and transparent middleware integration.

## Architecture Overview

The chaos engine is organized into seven primary modules:

| Module | Purpose |
|--------|---------|
| `fault` | Core trait system and fault abstractions |
| `context` | Request/response context for fault decisions |
| `registry` | Thread-safe fault instance management |
| `engine` | Central orchestration and coordination |
| `middleware` | Transparent fault injection layer |
| `scheduler` | Time-based fault sequencing |
| `config` | YAML/JSON configuration with validation |

## Core Abstractions

### Fault Trait

All faults implement the `Fault` async trait, which defines the contract for fault behavior:

```rust
#[async_trait]
pub trait Fault: Send + Sync {
    fn metadata(&self) -> &FaultMetadata;
    fn should_activate(&self, ctx: &FaultContext) -> bool;
    async fn apply(&self, ctx: &mut FaultContext) -> FaultBehavior;
    async fn before_operation(&self, ctx: &mut FaultContext) -> FaultBehavior;
    async fn after_operation(&self, ctx: &mut FaultContext) -> FaultBehavior;
    fn enable(&self);
    fn disable(&self);
    fn reset(&self);
    fn statistics(&self) -> FaultStatistics;
}
```

### FaultMetadata

Each fault carries metadata for identification, targeting, and categorization:

- **id, name, description**: Identification and documentation
- **category**: Network, Device, Protocol, Resource, or Other
- **severity**: Low, Medium, High, or Critical
- **probability**: Activation likelihood (0.0-1.0)
- **targets**: Glob patterns for device matching (e.g., `"modbus-*"`, `"*-sensor"`)
- **tags**: Categorization tags for filtering

### FaultBehavior

Determines operation behavior after fault application:

| Behavior | Effect |
|----------|--------|
| `Continue` | Operation proceeds normally |
| `Skip` | Operation is not executed |
| `Abort { error }` | Returns error immediately |
| `Delay { duration_ms }` | Sleeps before proceeding |
| `Modify` | Allows data modification |
| `Retry { max_attempts }` | Enables retry logic |
| `ReturnError { code, message }` | Returns specific error |

When multiple faults apply, behaviors merge with precedence: Abort > Skip > ReturnError > Delay > Modify > Continue.

### FaultContext

Carries operation metadata for fault activation decisions:

```rust
pub struct FaultContext {
    pub request_id: String,           // UUID for request tracking
    pub target: TargetInfo,           // Device information
    pub phase: RequestPhase,          // Before or After operation
    pub operation: OperationType,     // Read, Write, Initialize, etc.
    pub protocol: Protocol,           // ModbusTcp, OpcUa, BACnet, KNX
    pub timestamp: DateTime<Utc>,
    pub request_data: Option<RequestData>,
    pub response_data: Option<ResponseData>,
    pub applied_faults: Vec<AppliedFault>,
    pub accumulated_delay_ms: u64,
}
```

## Fault Categories

### Network Faults

#### NetworkLatencyFault

Injects configurable delays simulating network latency with multiple distribution models:

| Distribution | Description |
|--------------|-------------|
| `Constant` | Fixed delay value |
| `Uniform` | Random value within range (base ± jitter) |
| `Normal` | Gaussian distribution around base |
| `LogNormal` | Realistic network latency curves |
| `Exponential` | Exponential tail distribution |
| `Bimodal` | Two-state network simulation |

Configuration options:
- `base_ms`: Base latency in milliseconds
- `jitter_ms`: Variation range
- `min_ms` / `max_ms`: Clamping bounds
- `spike_probability`: Probability of latency spikes
- `spike_multiplier`: Magnitude of spikes

#### PacketLossFault

Simulates network packet loss with optional burst patterns:

- **Simple mode**: Random loss at fixed rate
- **Burst mode**: Correlated loss bursts of configurable length
- **Gilbert-Elliott model**: Markov chain with good/bad states

Configuration:
- `loss_rate`: Base loss rate (0.0-1.0)
- `burst`: Optional burst configuration (probability, min/max length)
- `correlation`: Gilbert-Elliott correlation factor

#### ConnectionFault

Simulates connection disruptions:
- Sudden disconnect
- Graceful close
- Connection reset
- Timeout

#### BandwidthFault

Throttles throughput to simulate congestion:
- `bytes_per_second`: Throughput limit
- `overhead_percent`: Protocol overhead percentage

### Device Faults

#### DeviceOfflineFault

Simulates device unavailability with multiple patterns:

| Pattern | Behavior |
|---------|----------|
| `Sudden` | Immediate offline transition |
| `Graceful` | Final message before offline |
| `Flapping` | Intermittent online/offline cycles |
| `Degraded` | Gradual slowdown before failure |
| `Scheduled` | Predictable maintenance windows |

#### SlowResponseFault

Injects response delays:
- `base_delay_ms`: Minimum delay
- `max_additional_delay_ms`: Random additional delay
- `response_timeout_ms`: Timeout threshold

#### CorruptedDataFault

Returns corrupted or invalid data:

| Strategy | Effect |
|----------|--------|
| Bit flip | Inverts bits in binary values |
| Zero/Max | Returns boundary values |
| Random | Returns random values |
| Bad quality | Sets quality flags to BAD |

#### StateTransitionFault

Fails device lifecycle transitions (Initialize, Start, Stop).

### Protocol Faults

#### MalformedPacketFault

Generates packets violating protocol specifications:
- Invalid header
- Incomplete payload
- Invalid field order
- Excessive payload
- Length mismatch

#### ChecksumFault

Injects invalid checksums or CRCs.

#### TimeoutFault

Simulates protocol-level timeouts with patterns:
- First request timeout
- Random timeouts
- All timeouts
- Cascading timeouts

#### ReorderFault

Returns responses out-of-order by buffering and shuffling.

## Fault Registry

The `FaultRegistry` provides thread-safe fault instance management using `DashMap`:

```rust
let registry = FaultRegistry::new();

// Register fault
registry.register("latency-001", latency_fault)?;

// Activate for specific targets
registry.activate("latency-001", "modbus-*")?;

// Query active faults
let active = registry.active_for("modbus-001");

// Filter faults
let filter = FaultFilter::new()
    .category(FaultCategory::Network)
    .min_severity(FaultSeverity::Medium)
    .enabled_only(true);
let results = registry.filter(&filter);
```

### Target Matching

The registry supports glob pattern matching:
- `"device-001"` - Exact match
- `"modbus-*"` - Prefix match
- `"*-sensor"` - Suffix match
- `"*bus*"` - Contains match
- `"*"` - Match all

## Chaos Engine

The `ChaosEngine` serves as the central orchestrator for fault injection:

```rust
let engine = ChaosEngine::builder()
    .add_fault("latency", latency_fault)
    .add_fault("loss", packet_loss_fault)
    .continue_on_error(true)
    .build();

// Engine lifecycle
engine.start().await?;
engine.enable("latency", "modbus-001").await?;

// Process operation
let mut ctx = FaultContext::builder()
    .device_id("modbus-001")
    .read("temperature")
    .protocol(Protocol::ModbusTcp)
    .build();

let behavior = engine.process(&mut ctx).await?;
```

### Engine State

```
Stopped <-> Running <-> Paused
```

### Engine Events

The engine emits events for observability:
- `Started`, `Stopped`, `Paused`, `Resumed`
- `FaultRegistered`, `FaultUnregistered`
- `FaultActivated`, `FaultDeactivated`
- `FaultApplied { fault_id, target, behavior }`
- `ScheduleEvent`
- `Error { message }`

## Chaos Middleware

The `ChaosMiddleware` provides transparent fault injection:

```rust
let middleware = ChaosMiddleware::new(engine);

// Pre-operation wrapping
let result = middleware.wrap_read("device-001", Protocol::ModbusTcp, "temp").await?;

match result {
    MiddlewareResult::Proceed(ctx) => {
        let response = device.read("temp").await?;
        middleware.process_response(ctx, response).await?
    }
    MiddlewareResult::Skip => { /* No response */ }
    MiddlewareResult::Error { code, message } => { /* Return error */ }
    MiddlewareResult::Delayed { delay_ms, result } => { /* Delayed response */ }
}
```

### Middleware Configuration

```rust
MiddlewareConfig {
    enabled: bool,              // Master switch
    inject_on_read: bool,       // Inject on read operations
    inject_on_write: bool,      // Inject on write operations
    inject_on_lifecycle: bool,  // Inject on init/start/stop
    verbose_logging: bool,      // Detailed logging
}
```

## Time-Based Scheduler

The `ChaosScheduler` orchestrates chaos events on a timeline:

```rust
let schedule = ChaosSchedule::builder()
    .name("resilience-test")
    .add_entry(
        ChaosEntry::builder()
            .start_secs(0.0)
            .duration_secs(60.0)
            .chaos_type(ChaosType::NetworkLatency { base_ms: 100, jitter_ms: 50 })
            .target("modbus-*")
            .intensity(1.0)
            .build()
    )
    .add_entry(
        ChaosEntry::builder()
            .start_secs(30.0)
            .duration_secs(30.0)
            .chaos_type(ChaosType::PacketLoss { rate: 0.1 })
            .target("*")
            .build()
    )
    .loop_schedule(true)
    .total_duration_secs(300.0)
    .build();

let scheduler = ChaosScheduler::new(schedule);
scheduler.start();

// Process ticks
loop {
    for event in scheduler.tick() {
        match event {
            ChaosEvent::Started(entry) => { /* Chaos began */ }
            ChaosEvent::Ended(entry) => { /* Chaos ended */ }
            ChaosEvent::ScheduleCompleted => break,
            ChaosEvent::ScheduleLooped => { /* Schedule restarted */ }
        }
    }
}
```

### Schedulable Chaos Types

| Type | Parameters |
|------|------------|
| `NetworkLatency` | base_ms, jitter_ms |
| `PacketLoss` | rate |
| `Disconnect` | - |
| `DeviceOffline` | - |
| `CorruptedData` | corruption_rate |
| `LoadSpike` | multiplier |
| `SlowResponse` | delay_ms |
| `Timeout` | timeout_ms |
| `MalformedPacket` | - |
| `InvalidChecksum` | - |
| `Custom` | fault_id |

## Configuration

The chaos engine supports YAML and JSON configuration:

```yaml
global:
  enabled: true
  default_probability: 1.0
  max_severity: critical
  verbose_logging: false
  target_patterns:
    - "modbus-*"
    - "bacnet-*"
  exclude_patterns:
    - "*-critical"
  dry_run: false
  seed: 12345  # For reproducibility

faults:
  network-latency:
    type: latency
    enabled: true
    probability: 0.8
    severity: medium
    targets:
      - "modbus-*"
    base_ms: 100
    jitter_ms: 50
    distribution: normal

  packet-loss:
    type: packet_loss
    enabled: true
    probability: 1.0
    severity: high
    targets:
      - "*"
    loss_rate: 0.05
    burst:
      probability: 0.1
      min_length: 2
      max_length: 5

schedules:
  - name: resilience-test
    enabled: true
    description: Test network resilience
    loop_schedule: true
    total_duration_secs: 300.0
    entries:
      - type: latency
        base_ms: 100
        jitter_ms: 50
        start_secs: 0.0
        duration_secs: 60.0
        targets:
          - "modbus-*"
        intensity: 1.0
```

### Loading Configuration

```rust
// From file
let config = ChaosConfig::from_yaml_file("chaos.yaml")?;
let config = ChaosConfig::from_json_file("chaos.json")?;

// From string
let config = ChaosConfig::from_yaml(yaml_string)?;

// Validation
config.validate()?;
```

## Statistics and Observability

### Fault Statistics

Each fault tracks runtime metrics:

```rust
pub struct FaultStatistics {
    pub checks_total: u64,       // Activation checks
    pub activations_total: u64,  // Successful activations
    pub skips_total: u64,        // Skipped due to probability
    pub errors_total: u64,       // Errors during application
    pub total_delay_ms: u64,     // Cumulative delay injected
    pub last_activation: Option<DateTime<Utc>>,
}

impl FaultStatistics {
    pub fn activation_rate(&self) -> f64;  // activations / checks
    pub fn error_rate(&self) -> f64;       // errors / activations
}
```

### Applied Faults Tracking

The `FaultContext` records all faults that executed:

```rust
ctx.applied_faults.iter().for_each(|applied| {
    println!("Fault: {}, Behavior: {:?}, At: {}",
        applied.fault_id, applied.behavior, applied.timestamp);
});

if ctx.was_affected() {
    println!("Total delay: {}ms", ctx.accumulated_delay_ms);
}
```

## Thread Safety

The chaos engine is designed for concurrent operation:

- `FaultRegistry` uses `DashMap` for lock-free reads
- `ChaosEngine` state protected by `Arc<RwLock<>>`
- All faults implement `Send + Sync`
- Async-first design with no blocking operations

## Error Handling

The `ChaosError` type provides comprehensive error categorization:

| Error | Recoverable | Description |
|-------|-------------|-------------|
| `InvalidConfig` | No | Configuration validation failure |
| `FaultNotFound` | No | Referenced fault does not exist |
| `FaultAlreadyActive` | Yes | Fault already activated for target |
| `EngineNotRunning` | No | Operation requires running engine |
| `InvalidStateTransition` | No | Invalid engine state change |
| `Timeout` | Yes | Operation timed out |

## BACnet-Specific Fault Types

The chaos engine includes a dedicated `bacnet` module with 5 protocol-aware fault types for building automation testing. All faults support probability-based activation, glob target matching, builder patterns, and severity levels.

### ApduFault

APDU-level corruption simulating wire-level protocol violations:

| Fault Type | Description |
|------------|-------------|
| `InvalidApduType` | Corrupt APDU type nibble (invalid values 8-15) |
| `CorruptInvokeId` | Replace invoke ID with forced or random value |
| `InvalidServiceChoice` | Replace service code with unsupported value (128-255) |
| `TruncatePayload` | Truncate APDU after header (incomplete messages) |
| `InvalidMaxApduLength` | Advertise impossibly small max-APDU-length |
| `DuplicateInvokeId` | Reuse recently-seen invoke IDs (TSM duplicate detection) |
| `GarbageApdu` | Replace entire APDU with random garbage bytes |
| `WrongSegmentationFlags` | Flip segmentation flag bits |

Configuration: Per-request or per-response direction control.

### ServiceFault

Application-layer service response manipulation:

| Fault Type | Description |
|------------|-------------|
| `RejectService` | Return REJECT PDU with configurable reason codes (0-9) |
| `AbortService` | Return ABORT PDU with reason codes (0-11) |
| `ErrorResponse` | Return ERROR PDU with configurable error class/code |
| `DropRequest` | Drop request entirely (triggers client timeout) |
| `WrongServiceResponse` | Return response for wrong service type |
| `SimpleAckInsteadOfComplex` | Return SimpleAck when ComplexAck expected |
| `EmptyComplexAck` | Return ComplexAck with empty payload |

Configuration: Target specific service choice codes.

### CovFault

COV (Change of Value) notification corruption:

| Fault Type | Description |
|------------|-------------|
| `DropNotification` | Block notification until subscription expires |
| `DelayNotification` | Delay notifications (creates stale data window) |
| `CorruptPresentValue` | Corrupt present-value (FixedValue, Noise, NaN, Negate, Zero, MaxValue) |
| `WrongProcessIdentifier` | Send notification with wrong subscriber process ID |
| `DuplicateNotification` | Send same notification twice |
| `UnsolicitedNotification` | Send notification for unsubscribed object |
| `IncompleteNotification` | Omit required properties (present-value, status-flags) |
| `WrongConfirmationStatus` | Flip confirmed/unconfirmed notification status |

Configuration: Target by object type, confirmed/unconfirmed filtering.

### PropertyFault

Property read/write response tampering:

| Fault Type | Description |
|------------|-------------|
| `WrongDataType` | Force wrong application tag (e.g., Unsigned instead of Real) |
| `OutOfRange` | Return out-of-range values (offset or fixed) |
| `StaleValue` | Cache-based staleness (return same value N times) |
| `FloatingPointCorruption` | NaN, PosInfinity, NegInfinity, Denormalized, NegativeZero |
| `PropertyAccessError` | Return BACnet error with class/code pair |
| `WrongPropertyId` | Return response for different property |
| `ArrayIndexError` | Return invalid-array-index error |
| `CorruptStatusFlags` | Flip status flag bits (in_alarm, fault, overridden, out_of_service) |

Configuration: Target by property ID and object type, read/write filtering.

### SegmentationFault

Segmented message handling corruption:

| Fault Type | Description |
|------------|-------------|
| `DropSegment` | Drop specific or random segment (triggers reassembly timeout) |
| `ReorderSegments` | Deliver segments in wrong order |
| `DuplicateSegment` | Send same segment twice |
| `WrongSequenceNumber` | Offset sequence number by configurable amount |
| `InvalidWindowSize` | Inject invalid window size value |
| `WrongMoreSegmentsFlag` | Incorrectly mark middle as last or vice versa |
| `OversizedSegment` | Exceed negotiated max-APDU-length |
| `WrongSegmentAckInvokeId` | Return Segment-Ack with wrong invoke ID |
| `InterSegmentDelay` | Delay between segments (timeout testing) |

Configuration: Minimum segment count filter, probability-based activation.

### BACnet Fault Module Structure

```
crates/mabi-chaos/src/bacnet/
├── mod.rs                  # Module hub and re-exports
├── apdu_fault.rs           # APDU-level corruption (8 fault types)
├── service_fault.rs        # Service-level failures (7 fault types)
├── cov_fault.rs            # COV notification faults (8 fault types)
├── property_fault.rs       # Property access faults (8 fault types)
└── segmentation_fault.rs   # Segmentation faults (9 fault types)
```

All BACnet fault types are exported via `mabi_chaos::prelude`.

## Dependencies

| Crate | Purpose |
|-------|---------|
| tokio | Async runtime and timing |
| async-trait | Async trait methods |
| serde, serde_yaml, serde_json | Configuration serialization |
| rand, rand_distr | Probability and distributions |
| dashmap | Thread-safe collections |
| parking_lot | Synchronization primitives |
| tracing | Structured logging |
| chrono | Timestamp handling |
| uuid | Unique identifier generation |
| thiserror | Error type derivation |
