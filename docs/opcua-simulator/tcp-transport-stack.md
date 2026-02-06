# OPC UA TCP Transport Stack

> Implementation of the TCP communication stack for the OPC UA binary protocol

## Overview

A production-grade TCP communication stack based on the OPC UA Part 6 (Mappings) specification has been added to the `mabi-opcua` crate. Binary encoding/decoding, TCP transport, Secure Channel management, and service dispatch layers have been integrated into the existing simulation-only server architecture, enabling the establishment of real TCP connections with OPC UA clients and the processing of service requests.

## Architecture

```text
┌──────────────────────────────────────────────────────────────────────────────────┐
│                              OpcUaServer                                         │
│                                                                                  │
│  ┌──────────────────────────────────────────────────────────────────────────┐    │
│  │                     Existing Server Components                           │    │
│  │  AddressSpace · SessionManager · SubscriptionManager                     │    │
│  │  HistoryStore · SecurityManager · NodeCache                              │    │
│  └──────────────────────────┬───────────────────────────────────────────────┘    │
│                             │ Shared via Arc                                     │
│  ┌──────────────────────────▼───────────────────────────────────────────────┐    │
│  │                     New TCP Transport Stack                              │    │
│  │                                                                          │    │
│  │  ┌─────────────┐   ┌───────────────┐   ┌──────────────────────────┐     │    │
│  │  │ TCP Listener │──▶│  Connection   │──▶│  Service Registry        │     │    │
│  │  │              │   │   Handler     │   │  (Dispatch → Handler)    │     │    │
│  │  └─────────────┘   └───────┬───────┘   └──────────────────────────┘     │    │
│  │                            │                                             │    │
│  │  ┌─────────────┐   ┌──────▼────────┐   ┌──────────────────────────┐     │    │
│  │  │  Transport   │   │   Secure      │   │  Binary Codec            │     │    │
│  │  │  Codec       │   │   Channel     │   │  (Encoder/Decoder)       │     │    │
│  │  │  (Framing)   │   │   Layer       │   │                          │     │    │
│  │  └─────────────┘   └──────────────┘   └──────────────────────────┘     │    │
│  │                                                                          │    │
│  │  ┌──────────────────────────────────────────────────────────────────┐    │    │
│  │  │                    Transport Metrics                              │    │    │
│  │  └──────────────────────────────────────────────────────────────────┘    │    │
│  └──────────────────────────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────────────────────────┘
```

## New Module Structure

| Module | Files | Role |
|--------|-------|------|
| [`codec`](#codec-module) | 6 | OPC UA binary encoding/decoding (Part 6, Section 5.2) |
| [`transport`](#transport-module) | 6 | TCP framing, connection management, listener |
| [`channel`](#channel-module) | 3 | Secure Channel management, security headers |
| [`service`](#service-module) | 8 | Service handler registry and individual handlers |

### Modified Existing Files

| File | Changes |
|------|---------|
| `error.rs` | 8 error variants added (`Codec`, `ProtocolError`, `ServiceNotSupported`, `BadSecureChannelId`, `BadSequenceNumber`, `MessageTooLarge`, `Bind`) |
| `lib.rs` | Module declarations for `codec`, `transport`, `channel`, `service` |
| `server.rs` | TCP listener creation/execution integration, `ServiceRegistry` wiring, `parse_endpoint_url()` function |

---

## Codec Module

> `crates/mabi-opcua/src/codec/`
>
> Binary encoding/decoding based on OPC UA Part 6, Section 5.2

### Traits

```rust
/// Binary encoding trait
pub trait BinaryEncodable {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()>;
    fn encoded_size(&self) -> usize;
}

/// Binary decoding trait
pub trait BinaryDecodable: Sized {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self>;
}
```

### File Details

#### `encoder.rs` --- Encoding Primitives

Types implementing `BinaryEncodable`:

| Type | Description |
|------|-------------|
| `bool`, `i8`, `u8` | 1 byte |
| `i16`, `u16` | 2 bytes, little-endian |
| `i32`, `u32` | 4 bytes, little-endian |
| `i64`, `u64` | 8 bytes, little-endian |
| `f32`, `f64` | IEEE 754, little-endian |
| `&str`, `String` | i32 length prefix + UTF-8 bytes |
| `Vec<u8>` | i32 length prefix + byte array |
| `DateTime<Utc>` | Windows FILETIME (i64) |
| `uuid::Uuid` | 16-byte UUID |

Utility functions:

```rust
pub fn encode_optional_string(s: &Option<String>, buf: &mut BytesMut) -> OpcUaResult<()>;
pub fn encode_optional_byte_string(bs: &Option<Vec<u8>>, buf: &mut BytesMut) -> OpcUaResult<()>;
pub fn encode_array<T: BinaryEncodable>(items: &[T], buf: &mut BytesMut) -> OpcUaResult<()>;
pub fn encode_optional_array<T: BinaryEncodable>(items: &Option<Vec<T>>, buf: &mut BytesMut) -> OpcUaResult<()>;
```

Constant: `FILETIME_UNIX_DIFF = 116_444_736_000_000_000` (offset between Unix epoch and Windows FILETIME)

#### `decoder.rs` --- Decoding Primitives

The `BinaryDecodable` implementation supports the same set of types as the encoder. During decoding, the `ensure_remaining()` function validates the remaining buffer length.

```rust
pub fn decode_optional_string(buf: &mut Bytes) -> OpcUaResult<Option<String>>;
pub fn decode_optional_byte_string(buf: &mut Bytes) -> OpcUaResult<Option<Vec<u8>>>;
pub fn decode_array<T: BinaryDecodable>(buf: &mut Bytes) -> OpcUaResult<Vec<T>>;
pub fn decode_optional_array<T: BinaryDecodable>(buf: &mut Bytes) -> OpcUaResult<Option<Vec<T>>>;
```

#### `variant.rs` --- Variant Binary Encoding

Variant encoding/decoding per OPC UA Part 6, Section 5.2.2.16. Serialization follows the type-ID byte + value format, with arrays distinguished by the `ARRAY_BIT (0x80)` flag.

#### `data_value.rs` --- DataValue and Composite Types

Selective field encoding based on an encoding mask byte:

| Mask Bit | Field |
|----------|-------|
| `0x01` | `value` (Variant) |
| `0x02` | `status` (StatusCode) |
| `0x04` | `source_timestamp` (DateTime) |
| `0x08` | `server_timestamp` (DateTime) |
| `0x10` | `source_picoseconds` (u16) |
| `0x20` | `server_picoseconds` (u16) |

Additionally includes encoding/decoding for the `QualifiedName`, `LocalizedText`, `StatusCode`, `ExtensionObject`, and `DiagnosticInfo` types.

```rust
pub struct ExtensionObject {
    pub type_id: NodeId,
    pub body: Option<Vec<u8>>,
}

pub struct DiagnosticInfo {
    pub symbolic_id: Option<i32>,
    pub namespace_uri: Option<i32>,
    pub locale: Option<i32>,
    pub localized_text: Option<i32>,
    pub additional_info: Option<String>,
    pub inner_status_code: Option<StatusCode>,
    pub inner_diagnostic_info: Option<Box<DiagnosticInfo>>,
}
```

#### `node_id.rs` --- NodeId Binary Encoding

Compact encoding per OPC UA Part 6, Section 5.2.2.9:

| Encoding Type | Bytes | Condition |
|---------------|-------|-----------|
| `TwoByte (0x00)` | 2 | ns=0, id 0--255 |
| `FourByte (0x01)` | 4 | ns 0--255, id 0--65535 |
| `Numeric (0x02)` | 7 | Full range |
| `String (0x03)` | Variable | String identifier |
| `Guid (0x04)` | 22 | UUID identifier |
| `ByteString (0x05)` | Variable | Byte array identifier |

---

## Transport Module

> `crates/mabi-opcua/src/transport/`
>
> TCP transport based on OPC UA Part 6, Section 7.1

### Message Types (`messages.rs`)

```rust
pub enum MessageType {
    Hello,              // HEL
    Acknowledge,        // ACK
    Error,              // ERR
    OpenSecureChannel,  // OPN
    CloseSecureChannel, // CLO
    Message,            // MSG
}

pub enum ChunkType {
    Final,        // 'F' --- Final chunk
    Intermediate, // 'C' --- Intermediate chunk
    Abort,        // 'A' --- Abort
}
```

#### MessageHeader

Fixed 8-byte header: `[type: 3 bytes][chunk: 1 byte][size: u32 LE]`

```rust
pub struct MessageHeader {
    pub message_type: MessageType,
    pub chunk_type: ChunkType,
    pub message_size: u32,
}
```

#### Hello / Acknowledge Handshake

```rust
pub struct HelloMessage {
    pub protocol_version: u32,    // Always 0
    pub receive_buffer_size: u32,
    pub send_buffer_size: u32,
    pub max_message_size: u32,
    pub max_chunk_count: u32,
    pub endpoint_url: String,
}

pub struct AcknowledgeMessage {
    pub protocol_version: u32,
    pub receive_buffer_size: u32,
    pub send_buffer_size: u32,
    pub max_message_size: u32,
    pub max_chunk_count: u32,
}
```

The `AcknowledgeMessage::from_hello()` method negotiates buffer sizes between the client's Hello message and the server's buffer capacity to produce an ACK.

Default values:

| Constant | Value | Description |
|----------|-------|-------------|
| `PROTOCOL_VERSION` | 0 | OPC UA protocol version |
| `DEFAULT_BUFFER_SIZE` | 65,535 | 64 KB |
| `DEFAULT_MAX_MESSAGE_SIZE` | 16,777,216 | 16 MB |
| `DEFAULT_MAX_CHUNK_COUNT` | 5,000 | Maximum chunk count |

### Transport Codec (`codec.rs`)

Implements `tokio_util::codec` `Encoder`/`Decoder` for framing messages on the TCP stream.

```rust
pub struct OpcUaTransportCodec {
    pub max_receive_buffer: u32,
    pub max_message_size: u32,
}

pub struct RawMessage {
    pub header: MessageHeader,
    pub body: Vec<u8>,
}
```

- `Decoder`: Parses the 8-byte header first, validates `message_size`, receives the complete message, then returns a `RawMessage`.
- `Encoder`: Concatenates the header and body for transmission.

### TCP Listener (`tcp_listener.rs`)

```rust
pub struct TcpTransportConfig {
    pub bind_address: SocketAddr,
    pub max_connections: usize,          // Default: 1000
    pub connection_timeout: Duration,    // Default: 60 seconds
    pub server_buffer_size: u32,         // Default: 65535
}

pub struct OpcUaTcpListener {
    config: TcpTransportConfig,
    service_registry: Arc<ServiceRegistry>,
    service_context: Arc<ServiceContextTemplate>,
    metrics: Arc<TransportMetrics>,
    shutdown: Arc<AtomicBool>,
    shutdown_tx: broadcast::Sender<()>,
}
```

Key methods:

| Method | Description |
|--------|-------------|
| `new(config, registry, context)` | Creates the listener |
| `run()` | Executes the TCP accept loop (blocking) |
| `shutdown()` | Sends the shutdown signal |
| `metrics()` | Returns a reference to transport metrics |

When accepting connections, if `max_connections` is exceeded, the connection is rejected and `metrics.record_rejection()` is recorded.

### Connection Handler (`connection.rs`)

Handles the complete OPC UA lifecycle for an individual TCP connection.

```rust
pub struct ServiceContextTemplate {
    pub session_manager: Arc<SessionManager>,
    pub address_space: Arc<AddressSpace>,
    pub subscription_manager: Arc<SubscriptionManager>,
    pub history_store: Arc<HistoryStore>,
    pub security_manager: Arc<SecurityManager>,
    pub server_config: Arc<OpcUaServerConfig>,
}
```

Connection processing flow:

```text
1. HEL (Hello) received → ACK (Acknowledge) response
2. OPN (OpenSecureChannel) received → SecureChannel created → OPN response
3. MSG (Message) receive/response loop:
   ├── SequenceHeader + Payload parsing
   ├── ServiceRegistry.dispatch() → Handler invocation
   └── Response encoding → MSG transmission
4. CLO (CloseSecureChannel) received → Connection closed
```

Internal helpers:

| Function | Purpose |
|----------|---------|
| `build_opn_response()` | Encodes OpenSecureChannelResponse |
| `build_service_fault()` | Generates a ServiceFault response (raw `NodeId + ResponseHeader` format) |
| `encode_error()` | Encodes error messages |

### Transport Metrics (`metrics.rs`)

All fields are `AtomicU64` lock-free counters.

```rust
pub struct TransportMetrics {
    pub connections_total: AtomicU64,
    pub connections_active: AtomicU64,
    pub connections_rejected: AtomicU64,
    pub messages_received: AtomicU64,
    pub messages_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub errors: AtomicU64,
}
```

---

## Channel Module

> `crates/mabi-opcua/src/channel/`
>
> OPC UA Secure Channel Layer

### SecureChannel (`secure_channel.rs`)

Responsible for Secure Channel creation, token issuance, and sequence number management. When `SecurityPolicy::None` is in effect, no actual cryptographic operations are performed.

```rust
pub struct SecureChannel {
    channel_id: u32,
    token_id: u32,
    security_policy: SecurityPolicy,
    security_mode: MessageSecurityMode,
    client_sequence_number: AtomicU32,
    server_sequence_number: AtomicU32,
    token_lifetime_ms: u32,
    token_created_at: Instant,
}
```

Channel IDs and token IDs are uniquely assigned via global `AtomicU32` counters.

| Method | Description |
|--------|-------------|
| `new_unsecured()` | Creates a channel with SecurityPolicy::None |
| `new(policy, mode, lifetime)` | Creates a channel with the specified security configuration |
| `next_server_sequence_number()` | Atomically increments the server sequence number |
| `validate_sequence_number(received)` | Validates the client sequence number |
| `renew_token(lifetime)` | Renews the security token |
| `is_token_expired()` | Checks whether the token has expired |

### Message Security Headers (`message.rs`)

Handles the security headers and sequence headers of OPN/MSG/CLO messages.

```rust
/// Asymmetric security header for OPN messages
pub struct AsymmetricSecurityHeader {
    pub security_policy_uri: String,
    pub sender_certificate: Vec<u8>,
    pub receiver_certificate_thumbprint: Vec<u8>,
}

/// Symmetric security header for MSG messages
pub struct SymmetricSecurityHeader {
    pub token_id: u32,
}

/// Sequence header (common to all secured messages)
pub struct SequenceHeader {
    pub sequence_number: u32,
    pub request_id: u32,
}
```

Functions for parsing and building OPN/MSG message bodies:

```rust
pub struct OpenSecureChannelBody { ... }
pub struct SecureMessageBody { ... }

pub fn build_opn_response_body(channel_id, seq_header, payload) -> Vec<u8>;
pub fn build_msg_response_body(channel_id, token_id, seq_header, payload) -> Vec<u8>;
```

---

## Service Module

> `crates/mabi-opcua/src/service/`
>
> Service handler layer based on OPC UA Part 4

### Service Registry (`registry.rs`)

Registers handlers keyed by request type NodeId and dispatches requests accordingly.

```rust
#[async_trait]
pub trait ServiceHandler: Send + Sync {
    fn request_type_id(&self) -> NodeId;
    async fn handle(&self, request_body: &[u8], context: &ServiceContext) -> OpcUaResult<ServiceResponse>;
}

pub struct ServiceRegistry {
    handlers: HashMap<NodeId, Arc<dyn ServiceHandler>>,
}

pub struct ServiceContext {
    pub session_manager: Arc<SessionManager>,
    pub address_space: Arc<AddressSpace>,
    pub subscription_manager: Arc<SubscriptionManager>,
    pub history_store: Arc<HistoryStore>,
    pub security_manager: Arc<SecurityManager>,
    pub server_config: Arc<OpcUaServerConfig>,
    pub channel: Arc<SecureChannel>,
    pub session_id: Option<NodeId>,
    pub auth_token: Option<NodeId>,
}

pub struct ServiceResponse {
    pub type_id: NodeId,
    pub body: Vec<u8>,
}
```

#### Dispatch Message Encoding Format

Per OPC UA Part 6, service requests and responses within MSG messages are encoded as raw `NodeId + body`, **not** as ExtensionObjects.

```text
Service request payload:
┌──────────────────────────────────────────┐
│  NodeId (request type_id)                │  ← e.g., i=428 (GetEndpoints)
│  RequestHeader + request body bytes      │  ← body passed to handler
└──────────────────────────────────────────┘

Service response payload:
┌──────────────────────────────────────────┐
│  NodeId (response type_id)               │  ← e.g., i=431 (GetEndpointsResponse)
│  ResponseHeader + response body bytes    │  ← body produced by handler
└──────────────────────────────────────────┘
```

The `dispatch()` method decodes the `NodeId` from the payload to locate the appropriate handler, then passes the remaining bytes after the NodeId as the `request_body` to the handler. The response is similarly constructed by directly concatenating `NodeId + body`.

> **Note**: The ExtensionObject format (`NodeId + encoding_byte + length + body`) is used only for internal fields such as `AdditionalHeader` and is not used for top-level service message wrapping. This distinction is defined in OPC UA Part 6, Section 6.7.3.

### Service Handler List

All handlers implement the `ServiceHandler` trait and are batch-registered in the registry via each module's `register_handlers()` function.

#### Discovery (`discovery.rs`) --- Part 4, Section 5.4

| Handler | Request ID | Response ID |
|---------|-----------|-------------|
| `GetEndpointsHandler` | 428 | 431 |

Common structures:
- `RequestHeader` --- Common header for all requests (authentication_token, timestamp, request_handle, timeout_hint, etc.)
- `ResponseHeader` --- Response header (timestamp, request_handle, service_result)
- `encode_application_description()` --- Encodes the server ApplicationDescription

#### Session (`session.rs`) --- Part 4, Section 5.6

| Handler | Request ID | Response ID |
|---------|-----------|-------------|
| `CreateSessionHandler` | 461 | 464 |
| `ActivateSessionHandler` | 467 | 470 |
| `CloseSessionHandler` | 473 | 476 |

#### Attribute (`attribute.rs`) --- Part 4, Section 5.10

| Handler | Request ID | Response ID |
|---------|-----------|-------------|
| `ReadHandler` | 631 | 634 |
| `WriteHandler` | 673 | 676 |

#### Browse (`browse.rs`) --- Part 4, Section 5.8

| Handler | Request ID | Response ID |
|---------|-----------|-------------|
| `BrowseHandler` | 527 | 530 |

#### Subscription (`subscription.rs`) --- Part 4, Section 5.13

| Handler | Request ID | Response ID |
|---------|-----------|-------------|
| `CreateSubscriptionHandler` | 787 | 790 |
| `DeleteSubscriptionsHandler` | 847 | 850 |
| `PublishHandler` | 826 | 829 |

#### MonitoredItem (`monitored_item.rs`) --- Part 4, Section 5.12

| Handler | Request ID | Response ID |
|---------|-----------|-------------|
| `CreateMonitoredItemsHandler` | 751 | 754 |
| `DeleteMonitoredItemsHandler` | 781 | 784 |

### Registration Flow

All handlers are registered in `OpcUaServer::create_tcp_listener()`:

```rust
let mut registry = ServiceRegistry::new();
service::discovery::register_handlers(&mut registry);
service::session::register_handlers(&mut registry);
service::attribute::register_handlers(&mut registry);
service::browse::register_handlers(&mut registry);
service::subscription::register_handlers(&mut registry);
service::monitored_item::register_handlers(&mut registry);
```

---

## Server Integration

The TCP listener has been integrated into the existing `OpcUaServer::start()` method:

```rust
pub async fn start(&self) -> OpcUaResult<()> {
    // ... existing initialization ...

    // Run TCP listener as a background task
    let tcp_listener = self.create_tcp_listener()?;
    tokio::spawn(async move {
        if let Err(e) = tcp_listener.run().await {
            warn!(error = %e, "TCP listener error");
        }
    });

    // ...
}
```

Inside `create_tcp_listener()`:
1. Parses `endpoint_url` (`opc.tcp://0.0.0.0:4840`) into a `SocketAddr`
2. Registers service handlers
3. Creates a `ServiceContextTemplate` (holding Arc references to server components)
4. Constructs the `OpcUaTcpListener`

---

## Error Type Additions

The following variants have been added to `OpcUaError`:

| Variant | Description |
|---------|-------------|
| `Codec(String)` | Binary encoding/decoding error |
| `ProtocolError(String)` | Protocol-level error |
| `ServiceNotSupported { service_id }` | Unsupported service request |
| `BadSecureChannelId(u32)` | Invalid Secure Channel ID |
| `BadSequenceNumber { expected, actual }` | Sequence number mismatch |
| `MessageTooLarge { size, max }` | Maximum message size exceeded |
| `Bind { address, reason }` | TCP bind failure |

---

## Design Patterns

| Pattern | Applied Location |
|---------|-----------------|
| **Trait-based serialization** | `BinaryEncodable` / `BinaryDecodable` |
| **Async service handlers** | `ServiceHandler` (async_trait) |
| **HashMap dispatch** | `ServiceRegistry` --- NodeId to Handler |
| **Atomic counters** | SecureChannel ID/Token ID, TransportMetrics |
| **Context template** | `ServiceContextTemplate` to per-connection `ServiceContext` |
| **tokio-util codec** | `OpcUaTransportCodec` --- Encoder/Decoder |

---

## Data Flow

```text
OPC UA Client
     │
     ▼
┌─────────────────┐
│  TCP Connection  │  ← tokio::net::TcpStream
└────────┬────────┘
         ▼
┌─────────────────┐
│  Transport Codec │  ← OpcUaTransportCodec (framing)
│  (8B header +   │
│   body)         │
└────────┬────────┘
         ▼
┌─────────────────┐
│  Connection     │  ← handle_connection()
│  Handler        │
│                 │  1. HEL → ACK
│                 │  2. OPN → SecureChannel → OPN Response
│                 │  3. MSG → Dispatch → MSG Response
│                 │  4. CLO → Close
└────────┬────────┘
         ▼
┌─────────────────┐
│  Secure Channel │  ← Sequence number validation, token management
│  + Message      │
│    Headers      │
└────────┬────────┘
         ▼
┌─────────────────┐
│  Service        │  ← ServiceRegistry.dispatch()
│  Registry       │     NodeId → type_id → Handler
└────────┬────────┘
         ▼
┌─────────────────┐
│  Service        │  ← ReadHandler, BrowseHandler, ...
│  Handler        │     Binary decode request → process → encode response
└────────┬────────┘
         ▼
┌─────────────────┐
│  Server         │  ← AddressSpace, SessionManager, ...
│  Components     │
└─────────────────┘
```
