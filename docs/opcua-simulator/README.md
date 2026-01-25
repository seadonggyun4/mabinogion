# mabi-opcua

OPC UA server simulator for the Mabinogion industrial protocol simulation framework.

## Overview

The `mabi-opcua` crate provides an OPC UA (Open Platform Communications Unified Architecture) server simulation environment. The implementation supports core OPC UA services including address space management, subscription-based data change notifications, and historical data access with aggregate computation.

## Architecture

### Server Components

```text
┌─────────────────────────────────────────────────────────────────────┐
│                         OpcUaServer                                  │
│  ┌──────────────┐  ┌──────────────────┐  ┌───────────────────────┐  │
│  │ AddressSpace │  │ SessionManager   │  │ SubscriptionManager   │  │
│  │  (DashMap)   │  │                  │  │                       │  │
│  └──────────────┘  └──────────────────┘  └───────────────────────┘  │
│         │                   │                       │               │
│  ┌──────┴──────┐     ┌──────┴──────┐        ┌──────┴──────┐        │
│  │  NodeCache  │     │  Security   │        │  History    │        │
│  │   (LRU)     │     │  Manager    │        │   Store     │        │
│  └─────────────┘     └─────────────┘        └─────────────┘        │
└─────────────────────────────────────────────────────────────────────┘
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
| [`address_space`](#address-space) | Node storage and namespace management |
| [`subscription`](#subscriptions) | Data change subscription and monitored item management |
| [`history`](#historical-access) | Historical data storage and aggregate computation |
| [`security`](#security) | Security policies and user authentication |
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
server.add_variable(node_id, name, value)?;
server.add_folder(node_id, name, parent_id)?;
```

#### Builder Pattern

```rust
use mabi_opcua::VariableBuilder;

let variable = VariableBuilder::new(node_id)
    .browse_name(namespace_index, name)
    .display_name(name)
    .description(description)
    .data_type(DataTypeId::Double)
    .value(initial_value)
    .writable()
    .historizing()
    .sampling_interval(1000)
    .build()?;
```

#### Variable Factory

Convenience methods for common data types:

```rust
use mabi_opcua::VariableFactory;

VariableFactory::boolean(node_id, name, value);
VariableFactory::boolean_writable(node_id, name, value);
VariableFactory::int32(node_id, name, value);
VariableFactory::float(node_id, name, value);
VariableFactory::double(node_id, name, value);
VariableFactory::double_writable(node_id, name, value);
VariableFactory::string(node_id, name, value);
VariableFactory::datetime(node_id, name, value);
```

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
// Write value to node
server.write_value(&node_id, new_value)?;
```

Writing a value:
- Updates the node's current value
- Records the value to history (if historizing is enabled)
- Notifies subscriptions asynchronously
- Broadcasts ValueChanged event
- Records metrics

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

```rust
use mabi_opcua::OpcUaDeviceFactory;
use mabi_core::factory::DeviceFactory;

let factory = OpcUaDeviceFactory;
let device = factory.create(device_config)?;
```

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
    // Server
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
    VariableBuilder,
    VariableFactory,
    BatchNodeCreator,

    // Types
    NodeId,
    DataValue,
    Variant,
    StatusCode,
    DataTypeId,

    // Subscriptions
    SubscriptionManager,
    DataChangeTrigger,
    DeadbandType,

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
    OpcUaDeviceFactory,
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
