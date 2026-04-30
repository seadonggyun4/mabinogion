# mabi-opcua

OPC UA server simulator for the Mabinogion industrial protocol simulation framework.

## Overview

The `mabi-opcua` crate provides an OPC UA (Open Platform Communications Unified Architecture) server simulation environment. The implementation supports core OPC UA services including address space management, subscription-based data change notifications, and historical data access with aggregate computation.

## Canonical Runtime Path

The preferred surface is now session-centric and file-backed:

- `OpcUaSimulatorConfig`
- `compile_session(...)`
- `mabi serve opcua --config <file> --session <name>`
- `mabi control opcua --config <file> --session <name> ...`

Canonical transports now support:

- `opc.tcp://...` as the default runtime path
- `https://...` as a protocol-aware canonical transport option when built with
  `mabi-opcua/https` or the CLI `opcua-https` feature
- `opc.tcp` Reverse Connect through named transport `connection_mode: reverse_connect`
- HTTPS Reverse Connect remains deferred to a later transport phase

Current documentation split:

- `simulator-config-spec.md`
- `simulator-control-plane-spec.md`
- `compat-migration.md`

Hand-built node builders and numeric `serve opcua` arguments have been removed from the
public compatibility surface. Use the canonical config/session path and the migration table
in `compat-migration.md` when moving older code forward. The migration guide remains in the
current release line only; the remaining legacy breadcrumbs are slated for removal in the
next major release.

Default validation is deterministic:

- `cargo test --workspace` is the canonical green path
- external interop is optional and runs through the repo-local container matrix
- perf threshold checks remain release-only ignored tests

## Architecture

### Server Components

```text
┌──────────────────────────────────────────────────────────────────────────┐
│                            OpcUaServer                                    │
│  ┌──────────────┐  ┌──────────────────┐  ┌───────────────────────┐       │
│  │ AddressSpace │  │ SessionManager   │  │ SubscriptionManager   │       │
│  │  (DashMap)   │  │                  │  │ (Data + Event)        │       │
│  └──────┬───────┘  └──────────────────┘  └───────────┬───────────┘       │
│         │                                            │                   │
│  ┌──────┴──────┐  ┌──────────────┐  ┌───────────────┴────────────┐      │
│  │  NodeCache  │  │  Security    │  │  History    │  EventManager│      │
│  │   (LRU)     │  │  Manager     │  │   Store     │              │      │
│  └─────────────┘  └──────────────┘  └─────────────┴──────────────┘      │
│                                                                          │
│  ┌──────────────┐  ┌──────────────────┐  ┌───────────────────────┐      │
│  │MethodRegistry│  │ ServiceRegistry  │  │ SecureChannel         │      │
│  │  (DashMap)   │  │ (Handler Dispatch│  │ (Token Renewal)       │      │
│  └──────────────┘  └──────────────────┘  └───────────────────────┘      │
└──────────────────────────────────────────────────────────────────────────┘
```

### Node Class Hierarchy

```text
                         BaseNode
                            │
        ┌───────────────────┼───────────────────┐
        │                   │                   │
   ObjectNode          VariableNode        MethodNode
        │                   │
   ObjectTypeNode     VariableTypeNode
                            │
                    ┌───────┴───────┐
                    │               │
             ReferenceTypeNode  DataTypeNode
                                    │
                                ViewNode
```

## Modules

| Module | Description |
|--------|-------------|
| [`server`](#server) | OPC UA server implementation with session management |
| [`address_space`](#address-space) | Node storage, namespace management, and browse continuation points |
| [`subscription`](#subscriptions) | Data change and event subscription with monitored item management |
| [`history`](#historical-access) | Historical data storage, aggregate computation, and HistoryRead service |
| [`event`](#event-system) | Event generation, filtering, and distribution (OPC UA Part 4, Section 7.17) |
| [`method`](#method-invocation) | Method registry, callback system, and Call service (OPC UA Part 4, Section 5.11) |
| [`browse`](#browse-services) | Browse, BrowseNext, and TranslateBrowsePathsToNodeIds services |
| [`security`](#security) | Security policies, user authentication, and secure channel token renewal |
| [`types`](#data-types) | OPC UA data types and node identifiers |
| [`cache`](#node-cache) | LRU caching for frequently accessed nodes |

## Server

The OPC UA server provides the main entry point for simulation:

```rust
use mabi_opcua::{OpcUaServer, OpcUaServerConfig};

let config = OpcUaServerConfig::default();
let server = OpcUaServer::new(config);
server.run().await?;
```

### Server Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `endpoint_url` | `opc.tcp://0.0.0.0:4840` | Server endpoint URL |
| `server_name` | `TRAP Simulator OPC UA Server` | Server display name |
| `security_policy` | `None` | Default security policy |
| `max_subscriptions` | 100 | Maximum concurrent subscriptions |
| `max_monitored_items` | 10,000 | Maximum monitored items per subscription |
| `min_publishing_interval_ms` | 100 | Minimum publishing interval |

### Builder Pattern

```rust
use mabi_opcua::OpcUaServerBuilder;

let server = OpcUaServerBuilder::new()
    .endpoint_url("opc.tcp://localhost:4840")
    .server_name("Test Server")
    .max_subscriptions(200)
    .build()?;
```

## Address Space

The address space stores all OPC UA nodes using a concurrent hash map (DashMap) for thread-safe access.

### Address Space Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `max_nodes` | 1,000,000 | Maximum nodes in address space |
| `max_references_per_node` | 10,000 | Maximum references per node |
| `enable_standard_namespace` | true | Create standard OPC UA namespace |
| `default_namespace_uri` | `urn:trap:simulator` | Default namespace URI |

### Standard Folders

The following standard folders are created when `enable_standard_namespace` is true:

| Node | NodeId | Description |
|------|--------|-------------|
| Root | ns=0;i=84 | Root folder |
| Objects | ns=0;i=85 | Container for object nodes |
| Types | ns=0;i=86 | Type definitions |
| Views | ns=0;i=87 | View definitions |
| Server | ns=0;i=2253 | Server diagnostics |

### Server Capabilities and Operation Limits

The address space includes standard `ServerCapabilities/OperationLimits` nodes (OPC UA Part 5, Section 6.3.2), which enable clients to discover per-request batch sizes:

| Node | NodeId | Default | Description |
|------|--------|---------|-------------|
| MaxNodesPerRead | ns=0;i=11565 | 0 (unlimited) | Maximum nodes in a Read request |
| MaxNodesPerWrite | ns=0;i=11567 | 0 (unlimited) | Maximum nodes in a Write request |
| MaxNodesPerBrowse | ns=0;i=11570 | 0 (unlimited) | Maximum nodes in a Browse request |
| MaxNodesPerMethodCall | ns=0;i=11569 | 0 (unlimited) | Maximum nodes in a Call request |
| MaxNodesPerRegisterNodes | ns=0;i=11571 | 0 (unlimited) | Maximum nodes in RegisterNodes |
| MaxNodesPerTranslateBrowsePathsToNodeIds | ns=0;i=11572 | 0 (unlimited) | Maximum paths per translate |
| MaxNodesPerNodeManagement | ns=0;i=11573 | 0 (unlimited) | Maximum nodes per management op |
| MaxMonitoredItemsPerCall | ns=0;i=11574 | 0 (unlimited) | Maximum monitored items per call |

The `HistoryServerCapabilities` node (ns=0;i=2330) exposes `AccessHistoryDataCapability` (ns=0;i=11192) indicating historical data access support.

### Data Type Hierarchy

The address space includes the standard OPC UA data type tree rooted at `BaseDataType` (ns=0;i=24):

```text
BaseDataType (i=24)
├── Boolean (i=1)
├── String (i=12)
├── DateTime (i=13)
├── Guid (i=14)
├── ByteString (i=15)
├── XmlElement (i=16)
├── NodeId (i=17)
├── ExpandedNodeId (i=18)
├── StatusCode (i=19)
├── QualifiedName (i=20)
├── LocalizedText (i=21)
├── Number (i=26)
│   ├── Integer (i=27)
│   │   ├── SByte (i=2) / Int16 (i=4) / Int32 (i=6) / Int64 (i=8)
│   │   └── UInteger (i=28)
│   │       └── Byte (i=3) / UInt16 (i=5) / UInt32 (i=7) / UInt64 (i=9)
│   ├── Float (i=10)
│   └── Double (i=11)
├── Structure (i=22)
└── Enumeration (i=29)
```

All nodes are connected via `HasSubtype` references, enabling proper type hierarchy traversal for `OfType` filter operators and `IsAbstract` attribute resolution.

### Event Type Hierarchy

Standard event types rooted at `BaseEventType` (ns=0;i=2041):

| Event Type | NodeId | Description |
|------------|--------|-------------|
| BaseEventType | ns=0;i=2041 | Abstract root event type |
| AuditEventType | ns=0;i=2052 | Audit trail events |
| SystemEventType | ns=0;i=2130 | System-level events |
| DeviceFailureEventType | ns=0;i=2131 | Device failure events |
| BaseModelChangeEventType | ns=0;i=2132 | Model change notifications |

Standard event properties are exposed as `HasProperty` references from `BaseEventType`:

| Property | NodeId | Data Type | Description |
|----------|--------|-----------|-------------|
| EventId | ns=0;i=2042 | ByteString | Unique event identifier |
| EventType | ns=0;i=2043 | NodeId | Event type |
| SourceNode | ns=0;i=2044 | NodeId | Source node |
| SourceName | ns=0;i=2045 | String | Source name |
| Time | ns=0;i=2046 | DateTime | Event timestamp |
| ReceiveTime | ns=0;i=2047 | DateTime | Server receipt time |
| Message | ns=0;i=2050 | LocalizedText | Human-readable message |
| Severity | ns=0;i=2051 | UInt16 | Event severity (0-1000) |

### Browse Continuation Points

The `browse_next()` method supports paginated browsing of large result sets:

- Continuation points are created automatically when results exceed `max_references_returned`
- Points expire after 5 minutes if not consumed
- `release_continuation_point()` explicitly frees allocated state
- Points are stored per-session in the address space

### Node Classes

Seven node classes are supported per the OPC UA specification:

| Class | Description |
|-------|-------------|
| `ObjectNode` | Container for variables and methods |
| `VariableNode` | Holds readable/writable values |
| `MethodNode` | Callable functions |
| `ObjectTypeNode` | Type definitions for objects |
| `VariableTypeNode` | Type definitions for variables |
| `ReferenceTypeNode` | Relationship type definitions |
| `DataTypeNode` | Data type definitions |
| `ViewNode` | Subsets of the address space |

### Node Creation

#### Simple API

```rust
server.add_variable(node_id, name, value)?;          // Read-only variable
server.add_writable_variable(node_id, name, value)?;  // Read/write variable
server.add_folder(node_id, name, parent_id)?;
```

`add_variable` creates a node with `AccessLevel::CURRENT_READ`. `add_writable_variable` creates a node with `AccessLevel::READ_WRITE`, allowing OPC UA clients to write values via the Write service.

#### Builder Pattern

The builder-oriented compatibility surface has been removed. Model the same node shape via
`OpcUaSimulatorConfig.models` overlays or `PresetDefinition`, then compile with
`compile_session(...)`.

#### Variable Factory

Factory helpers have been removed from the public surface. Prefer typed `DeviceDefinition`
bindings and generated catalog materialization.

#### Batch Creation

For creating large numbers of nodes:

```rust
use mabi_opcua::{BatchNodeCreator, ProgressCallback};

struct MyProgressCallback;
impl ProgressCallback for MyProgressCallback {
    fn on_progress(&self, created: usize, total: usize) {
        println!("Created {}/{} nodes", created, total);
    }
}

let creator = BatchNodeCreator::new()
    .with_progress_callback(MyProgressCallback);

creator.create_nodes(&templates).await?;
```

### Node Identifier Types

Four identifier types are supported:

| Type | Example | Description |
|------|---------|-------------|
| Numeric | `NodeId::numeric(1, 1000)` | Namespace index + numeric ID |
| String | `NodeId::string(1, "Temperature")` | Namespace index + string ID |
| GUID | `NodeId::guid(1, uuid)` | Namespace index + UUID |
| ByteString | `NodeId::byte_string(1, bytes)` | Namespace index + byte array |

### Namespace Management

```rust
// Register custom namespace
let ns_index = server.register_namespace("urn:mycompany:mydevice")?;

// Create node in custom namespace
let node_id = NodeId::numeric(ns_index, 1000);
```

## Data Types

### Supported Data Types

| Type | Description |
|------|-------------|
| Null | Null value |
| Boolean | True/false |
| SByte | Signed 8-bit integer |
| Byte | Unsigned 8-bit integer |
| Int16 | Signed 16-bit integer |
| UInt16 | Unsigned 16-bit integer |
| Int32 | Signed 32-bit integer |
| UInt32 | Unsigned 32-bit integer |
| Int64 | Signed 64-bit integer |
| UInt64 | Unsigned 64-bit integer |
| Float | IEEE 754 single precision |
| Double | IEEE 754 double precision |
| String | UTF-8 string |
| DateTime | Date and time with picosecond precision |
| Guid | UUID |
| ByteString | Byte array |
| XmlElement | XML data |
| NodeId | Node identifier |
| ExpandedNodeId | Node identifier with namespace URI |
| StatusCode | Quality indicator |
| QualifiedName | Namespace-qualified name |
| LocalizedText | Localized text with locale identifier |
| ExtensionObject | Structured data |
| DataValue | Value with metadata |
| Variant | Union of all types |
| DiagnosticInfo | Diagnostic information |

### Array Support

Both scalar and array values are supported:

```rust
// Scalar value
let scalar = Variant::Double(25.5);

// Array value
let array = Variant::DoubleArray(vec![1.0, 2.0, 3.0]);
```

### DataValue Structure

```rust
pub struct DataValue {
    pub value: Option<Variant>,
    pub status: StatusCode,
    pub source_timestamp: Option<DateTime<Utc>>,
    pub source_picoseconds: u16,
    pub server_timestamp: Option<DateTime<Utc>>,
    pub server_picoseconds: u16,
}
```

## Subscriptions

The subscription system provides data change notifications to clients.

### Subscription Management

```rust
use mabi_opcua::{SubscriptionManager, Subscription};

let manager = SubscriptionManager::new();

// Create subscription
let subscription_id = manager.create_subscription(
    publishing_interval_ms,
    lifetime_count,
    max_keepalive_count,
)?;

// Add monitored item
manager.add_monitored_item(
    subscription_id,
    node_id,
    sampling_interval_ms,
    trigger,
    deadband,
)?;
```

### Data Change Triggers

| Trigger | Description |
|---------|-------------|
| `Status` | Notify on status code change |
| `Value` | Notify on value change (with optional deadband) |
| `StatusOrValue` | Notify on either status or value change |

### Deadband Filtering

| Type | Description |
|------|-------------|
| `None` | No filtering, notify on every change |
| `Absolute` | Absolute threshold (e.g., 0.5 units) |
| `Percent` | Percentage of value range (e.g., 2%) |

### Monitored Item Configuration

| Parameter | Description |
|-----------|-------------|
| `node_id` | Node to monitor |
| `sampling_interval` | How often to sample (ms) |
| `trigger` | Data change trigger mode |
| `deadband_type` | Deadband filter type |
| `deadband_value` | Deadband threshold |
| `queue_size` | Notification queue size |

## Historical Access

The history store provides raw data storage and aggregate computation.

### History Store Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `max_values_per_node` | 100,000 | Maximum historical values per node |
| `max_age_seconds` | 2,592,000 (30 days) | Maximum age of stored values |
| `default_batch_size` | 1,000 | Default query batch size |
| `max_batch_size` | 10,000 | Maximum query batch size |
| `auto_cleanup` | true | Automatic cleanup of old values |
| `cleanup_interval_seconds` | 3,600 (1 hour) | Cleanup interval |
| `enable_compression` | false | Enable deadband compression |

### Raw History Reading

```rust
use mabi_opcua::HistoryStore;

let history = HistoryStore::new(config);

// Read raw historical data
let values = history.read_raw(
    &node_id,
    start_time,
    end_time,
    max_values,
)?;
```

### Aggregate Functions

The following aggregate functions are supported:

| Category | Functions |
|----------|-----------|
| Statistics | Average, TimeAverage, Total, Minimum, Maximum, Range, StandardDeviation, Variance |
| Time-based | DurationInState, NumberOfTransitions |
| Quality | PercentGood, PercentBad, WorstQuality |
| State | Start, End, Delta, Count |
| Advanced | Interpolative, AnnotationCount |

```rust
use mabi_opcua::AggregateType;

let result = history.read_aggregate(
    &node_id,
    start_time,
    end_time,
    AggregateType::Average,
    processing_interval,
)?;
```

## Event System

The event system (OPC UA Part 4, Section 7.17) provides full event generation, filtering, and distribution to subscriptions.

### EventManager

The `EventManager` processes events and delivers them to all interested subscriptions:

```rust
use mabi_opcua::EventManager;

let event = EventData::new(
    NodeId::numeric(0, 2041),  // BaseEventType
    NodeId::numeric(2, 1001),  // Source node
    "TemperatureSensor",       // Source name
    500,                       // Severity
    "Temperature exceeded threshold",
)
.with_field(
    vec![QualifiedName::new(2, "CurrentValue")],
    Variant::Double(85.7),
);

event_manager.fire_event(event).await;
```

### Event Filter

Event subscriptions use `EventFilter` to specify which fields to return and which events to accept:

```text
EventFilter (encoding_id i=725)
├── SelectClauses: SimpleAttributeOperand[]
│   ├── TypeDefinitionId (NodeId)
│   ├── BrowsePath (QualifiedName[])
│   ├── AttributeId (u32)
│   └── IndexRange (String)
└── WhereClause: ContentFilterElement[]
    ├── FilterOperator (u32 enum)
    └── FilterOperands: ExtensionObject[]
```

### Content Filter Operators

The where clause supports the following operators for event filtering:

| Operator | Value | Description |
|----------|-------|-------------|
| Equals | 0 | Equality comparison |
| IsNull | 1 | Null check |
| GreaterThan | 2 | Greater than comparison |
| LessThan | 3 | Less than comparison |
| GreaterThanOrEqual | 4 | Greater than or equal |
| LessThanOrEqual | 5 | Less than or equal |
| Like | 6 | Pattern matching |
| Not | 7 | Logical negation |
| Between | 8 | Range check |
| InList | 9 | Set membership |
| And | 10 | Logical AND |
| Or | 11 | Logical OR |
| Cast | 12 | Type cast |
| InView | 13 | View membership |
| OfType | 14 | Type hierarchy check |
| RelatedTo | 15 | Reference relationship |
| BitwiseAnd | 16 | Bitwise AND |
| BitwiseOr | 17 | Bitwise OR |

The `OfType` operator traverses the `HasSubtype` hierarchy in the address space to evaluate event type membership, supporting up to 50 levels of inheritance depth.

### Monitored Item Kinds

Monitored items are classified by their subscription mode:

| Kind | Description | Filter Type |
|------|-------------|-------------|
| DataChange | Monitors node value changes | DataChangeFilter (deadband, trigger) |
| Event | Monitors events from source nodes | EventFilter (select + where clauses) |

## Method Invocation

The Call service (OPC UA Part 4, Section 5.11.2) enables remote procedure call via the `MethodRegistry`:

```rust
use mabi_opcua::MethodRegistry;

let registry = MethodRegistry::new();

// Register a method with callback
registry.register(
    object_node_id,
    method_node_id,
    Arc::new(|inputs: &[Variant]| -> Result<Vec<Variant>, StatusCode> {
        let a = inputs[0].as_f64().unwrap_or(0.0);
        let b = inputs[1].as_f64().unwrap_or(0.0);
        Ok(vec![Variant::Double(a * b)])
    }),
);
```

The `CallHandler` validates object and method node existence, maps input arguments to registered callbacks, and returns output arguments or appropriate status codes. Unregistered methods return `BadNotImplemented`.

## Browse Services

### TranslateBrowsePathsToNodeIds

Resolves relative browse paths from starting nodes (OPC UA Part 4, Section 5.8.4):

- Walks the address space following `RelativePathElement` arrays
- Supports hierarchical reference type filtering
- Handles inverse reference traversal
- Returns `ExpandedNodeId` results with server index and namespace URI

### RegisterNodes / UnregisterNodes

Pass-through implementation per OPC UA Part 4, Sections 5.8.5 and 5.8.6. The server returns the same NodeIds received in the request, which is spec-compliant for non-optimizing servers.

### TransferSubscriptions

Enables session recovery by transferring active subscriptions between sessions (OPC UA Part 4, Section 5.13.7):

- Returns available sequence numbers for each transferred subscription
- Status: `GoodSubscriptionTransferred` or `BadSubscriptionIdInvalid`

## OPC UA Service Compliance Matrix

| Service | OPC UA Part 4 Section | Status | Notes |
|---------|----------------------|--------|-------|
| FindServers | 5.4.2 | Supported | Discovery service |
| GetEndpoints | 5.4.4 | Supported | Endpoint discovery |
| CreateSession | 5.6.2 | Supported | Session management |
| ActivateSession | 5.6.3 | Supported | Session activation |
| CloseSession | 5.6.4 | Supported | Session cleanup |
| Read | 5.10.2 | Supported | Attribute read |
| Write | 5.10.4 | Supported | Attribute write with AccessLevel check |
| HistoryRead | 5.10.3 | **New** | ReadRaw, ReadProcessed, continuation points |
| Browse | 5.8.2 | Enhanced | Continuation points, subtype filtering |
| BrowseNext | 5.8.3 | **New** | Pagination of browse results |
| TranslateBrowsePathsToNodeIds | 5.8.4 | **New** | Full path resolution |
| RegisterNodes | 5.8.5 | **New** | Pass-through implementation |
| UnregisterNodes | 5.8.6 | **New** | Pass-through implementation |
| Call | 5.11.2 | **New** | Method registry and invocation |
| CreateSubscription | 5.13.1 | Supported | Data change subscriptions |
| ModifySubscription | 5.13.2 | Supported | Subscription parameter update |
| DeleteSubscriptions | 5.13.4 | Supported | Subscription cleanup |
| Publish | 5.13.5 | Enhanced | DataChange + Event notifications |
| CreateMonitoredItems | 5.12.2 | Enhanced | EventFilter parsing support |
| TransferSubscriptions | 5.13.7 | **New** | Session recovery |
| OpenSecureChannel | Part 6 Section 6.7.1 | Enhanced | Issue + Renew request types |

## Secure Channel Token Renewal

The transport layer supports in-band secure channel token renewal (OPC UA Part 6, Section 6.7.4):

- Handles OPN (OpenSecureChannel) messages within the active service message loop
- Implements `channel.renew_token(lifetime)` for token refresh
- Updates `token_id` while preserving `channel_id` for long-lived connections
- Interior mutability via `RwLock` for thread-safe token state management

## Security

### Security Policies

| Policy | Description |
|--------|-------------|
| `None` | No security |
| `Basic128Rsa15` | AES-128 + RSA 1024-bit |
| `Basic256` | AES-256 + RSA 1024-bit |
| `Basic256Sha256` | AES-256 + RSA 2048-bit + SHA-256 |
| `Aes128Sha256RsaOaep` | AES-128 + RSA-OAEP + SHA-256 |
| `Aes256Sha256RsaPss` | AES-256 + RSA-PSS + SHA-256 |

### Message Security Modes

| Mode | Description |
|------|-------------|
| `None` | No protection |
| `Sign` | Message signing only |
| `SignAndEncrypt` | Both signing and encryption |

The CLI enforces strict validation of the `--security` parameter via a `ValueEnum` constraint. Only the three modes listed above are accepted; invalid values are rejected at the argument parsing layer prior to server initialization. The matching is case-insensitive (e.g., `sign`, `Sign`, and `SIGN` are all accepted).

### User Authentication

| Type | Description |
|------|-------------|
| Anonymous | No authentication required |
| Username/Password | Username and password credentials |
| X.509 Certificate | Client certificate authentication |
| Issued Token | JWT or similar token-based authentication |

## Node Cache

LRU caching for frequently accessed nodes:

### Cache Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `max_size` | 100,000 | Maximum cached nodes |
| `prefetch_enabled` | true | Enable prefetching of related nodes |
| `prefetch_depth` | 2 | Levels of references to prefetch |
| `cache_values` | true | Cache node values |
| `value_cache_ttl_ms` | 1,000 | Value cache time-to-live |

## Value Operations

### Writing Values

```rust
// Write value to node (node must have AccessLevel::CURRENT_WRITE)
server.write_value(&node_id, new_value)?;
```

Writing a value:
- Checks `AccessLevel` — returns `BadNotWritable` (0x803B0000) if the node lacks write permission
- Updates the node's current value
- Records the value to history (if historizing is enabled)
- Notifies subscriptions asynchronously
- Broadcasts ValueChanged event
- Records metrics

To create writable nodes, use `server.add_writable_variable()` or the `VariableBuilder::writable()` method.

### Reading Values

```rust
let data_value = server.read_value(&node_id)?;
```

## Scenario Engine Integration

### Device Trait Implementation

The `OpcUaDevice` struct implements the `mabi_core::device::Device` trait:

```rust
pub struct OpcUaDevice {
    info: DeviceInfo,
    point_defs: HashMap<String, DataPointDefinition>,
    values: RwLock<HashMap<String, DataPointValue>>,
    stats: RwLock<DeviceStatistics>,
    event_tx: broadcast::Sender<DataPoint>,
}
```

### Device Factory

Legacy device factory builders have been removed from the public surface. Use file-backed
`DeviceDefinition` entries and compile them into a session catalog instead.

### Device Metadata

Scenario configuration supports OPC UA-specific metadata:

```rust
pub struct OpcUaDeviceMetadata {
    server_config: OpcUaServerConfig,
    address_space_config: Option<AddressSpaceConfig>,
    cache_config: Option<NodeCacheConfig>,
    history_config: Option<HistoryStoreConfig>,
    namespace_uri: String,
    namespace_index: u16,
}
```

## Error Handling

```rust
pub enum OpcUaError {
    NodeNotFound { node_id: String },
    InvalidNodeId(String),
    Server(String),
    Connection(String),
    Subscription(String),
    Security(String),
    InvalidState(String),
    WriteError(String),
    Io(std::io::Error),
    Core(mabi_core::Error),
}
```

## Public API

### Core Types

```rust
pub use mabi_opcua::{
    // Server (add_variable, add_writable_variable, add_folder)
    OpcUaServer,
    OpcUaServerBuilder,
    OpcUaServerConfig,

    // Address Space
    AddressSpace,
    AddressSpaceConfig,

    // Nodes
    ObjectNode,
    VariableNode,
    MethodNode,
    BatchNodeCreator,

    // Types
    NodeId,
    DataValue,
    Variant,
    StatusCode,
    DataTypeId,
    QualifiedName,
    ExpandedNodeId,

    // Subscriptions
    SubscriptionManager,
    DataChangeTrigger,
    DeadbandType,

    // Events
    EventManager,
    EventData,
    EventFilter,
    EventFieldList,
    ContentFilterElement,
    FilterOperator,
    SimpleAttributeOperand,

    // Method Invocation
    MethodRegistry,
    CallHandler,

    // History
    HistoryStore,
    HistoryStoreConfig,
    AggregateType,

    // Security
    SecurityPolicy,
    MessageSecurityMode,
    UserTokenType,

    // Cache
    NodeCache,
    NodeCacheConfig,

    // Device Integration
    OpcUaDevice,
    OpcUaDeviceMetadata,

    // Error Handling
    OpcUaError,
    OpcUaResult,
};
```

## Testing

```bash
# Run all tests
cargo test --package mabi-opcua

# Run specific module tests
cargo test --package mabi-opcua address_space::
cargo test --package mabi-opcua subscription::

# Run with output
cargo test --package mabi-opcua -- --nocapture
```
