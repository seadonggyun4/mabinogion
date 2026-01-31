# OPC UA TCP Transport Stack

> OPC UA 바이너리 프로토콜의 실제 TCP 통신 스택 구현

## Overview

`mabi-opcua` 크레이트에 OPC UA Part 6 (Mappings) 명세를 기반으로 한 실제 TCP 통신 스택이 추가되었습니다. 기존의 시뮬레이션 전용 서버 구조에 바이너리 인코딩/디코딩, TCP 트랜스포트, Secure Channel 관리, 서비스 디스패치 계층이 통합되어 OPC UA 클라이언트와 실제 TCP 연결을 수립하고 서비스 요청을 처리할 수 있습니다.

## Architecture

```text
┌──────────────────────────────────────────────────────────────────────────────────┐
│                              OpcUaServer                                         │
│                                                                                  │
│  ┌──────────────────────────────────────────────────────────────────────────┐    │
│  │                        기존 서버 컴포넌트                                 │    │
│  │  AddressSpace · SessionManager · SubscriptionManager                     │    │
│  │  HistoryStore · SecurityManager · NodeCache                              │    │
│  └──────────────────────────┬───────────────────────────────────────────────┘    │
│                             │ Arc 공유                                           │
│  ┌──────────────────────────▼───────────────────────────────────────────────┐    │
│  │                    신규 TCP Transport Stack                               │    │
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

## 신규 모듈 구성

| 모듈 | 파일 수 | 역할 |
|------|---------|------|
| [`codec`](#codec-모듈) | 6 | OPC UA 바이너리 인코딩/디코딩 (Part 6 Section 5.2) |
| [`transport`](#transport-모듈) | 6 | TCP 프레이밍, 연결 관리, 리스너 |
| [`channel`](#channel-모듈) | 3 | Secure Channel 관리, 보안 헤더 |
| [`service`](#service-모듈) | 8 | 서비스 핸들러 레지스트리 및 개별 핸들러 |

### 변경된 기존 파일

| 파일 | 변경 내용 |
|------|----------|
| `error.rs` | 에러 variant 8개 추가 (`Codec`, `ProtocolError`, `ServiceNotSupported`, `BadSecureChannelId`, `BadSequenceNumber`, `MessageTooLarge`, `Bind`) |
| `lib.rs` | `codec`, `transport`, `channel`, `service` 모듈 선언 |
| `server.rs` | TCP 리스너 생성/실행 통합, `ServiceRegistry` 연동, `parse_endpoint_url()` 함수 |

---

## Codec 모듈

> `crates/mabi-opcua/src/codec/`
>
> OPC UA Part 6, Section 5.2 기반 바이너리 인코딩/디코딩

### 트레이트

```rust
/// 바이너리 인코딩 트레이트
pub trait BinaryEncodable {
    fn encode(&self, buf: &mut BytesMut) -> OpcUaResult<()>;
    fn encoded_size(&self) -> usize;
}

/// 바이너리 디코딩 트레이트
pub trait BinaryDecodable: Sized {
    fn decode(buf: &mut Bytes) -> OpcUaResult<Self>;
}
```

### 파일별 상세

#### `encoder.rs` — 인코딩 프리미티브

`BinaryEncodable` 구현 대상:

| 타입 | 설명 |
|------|------|
| `bool`, `i8`, `u8` | 1바이트 |
| `i16`, `u16` | 2바이트, little-endian |
| `i32`, `u32` | 4바이트, little-endian |
| `i64`, `u64` | 8바이트, little-endian |
| `f32`, `f64` | IEEE 754, little-endian |
| `&str`, `String` | i32 길이 + UTF-8 바이트 |
| `Vec<u8>` | i32 길이 + 바이트 배열 |
| `DateTime<Utc>` | Windows FILETIME (i64) |
| `uuid::Uuid` | 16바이트 UUID |

유틸리티 함수:

```rust
pub fn encode_optional_string(s: &Option<String>, buf: &mut BytesMut) -> OpcUaResult<()>;
pub fn encode_optional_byte_string(bs: &Option<Vec<u8>>, buf: &mut BytesMut) -> OpcUaResult<()>;
pub fn encode_array<T: BinaryEncodable>(items: &[T], buf: &mut BytesMut) -> OpcUaResult<()>;
pub fn encode_optional_array<T: BinaryEncodable>(items: &Option<Vec<T>>, buf: &mut BytesMut) -> OpcUaResult<()>;
```

상수: `FILETIME_UNIX_DIFF = 116_444_736_000_000_000` (Unix epoch ↔ Windows FILETIME 오프셋)

#### `decoder.rs` — 디코딩 프리미티브

`BinaryDecodable` 구현은 인코더와 동일한 타입 세트를 지원합니다. 디코딩 시 `ensure_remaining()` 함수로 버퍼 잔량을 검증합니다.

```rust
pub fn decode_optional_string(buf: &mut Bytes) -> OpcUaResult<Option<String>>;
pub fn decode_optional_byte_string(buf: &mut Bytes) -> OpcUaResult<Option<Vec<u8>>>;
pub fn decode_array<T: BinaryDecodable>(buf: &mut Bytes) -> OpcUaResult<Vec<T>>;
pub fn decode_optional_array<T: BinaryDecodable>(buf: &mut Bytes) -> OpcUaResult<Option<Vec<T>>>;
```

#### `variant.rs` — Variant 바이너리 인코딩

OPC UA Part 6, Section 5.2.2.16에 따른 Variant 인코딩/디코딩. 타입 ID 바이트 + 값 방식으로 직렬화하며, 배열은 `ARRAY_BIT (0x80)` 플래그로 구분합니다.

#### `data_value.rs` — DataValue 및 복합 타입

마스크 바이트 기반 선택적 필드 인코딩:

| 마스크 비트 | 필드 |
|------------|------|
| `0x01` | `value` (Variant) |
| `0x02` | `status` (StatusCode) |
| `0x04` | `source_timestamp` (DateTime) |
| `0x08` | `server_timestamp` (DateTime) |
| `0x10` | `source_picoseconds` (u16) |
| `0x20` | `server_picoseconds` (u16) |

추가로 `QualifiedName`, `LocalizedText`, `StatusCode`, `ExtensionObject`, `DiagnosticInfo` 타입의 인코딩/디코딩을 포함합니다.

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

#### `node_id.rs` — NodeId 바이너리 인코딩

OPC UA Part 6, Section 5.2.2.9에 따른 컴팩트 인코딩:

| 인코딩 타입 | 바이트 | 조건 |
|------------|--------|------|
| `TwoByte (0x00)` | 2 | ns=0, id 0–255 |
| `FourByte (0x01)` | 4 | ns 0–255, id 0–65535 |
| `Numeric (0x02)` | 7 | 전체 범위 |
| `String (0x03)` | 가변 | 문자열 식별자 |
| `Guid (0x04)` | 22 | UUID 식별자 |
| `ByteString (0x05)` | 가변 | 바이트 배열 식별자 |

---

## Transport 모듈

> `crates/mabi-opcua/src/transport/`
>
> OPC UA Part 6, Section 7.1 기반 TCP 트랜스포트

### 메시지 타입 (`messages.rs`)

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
    Final,        // 'F' — 최종 청크
    Intermediate, // 'C' — 중간 청크
    Abort,        // 'A' — 중단
}
```

#### MessageHeader

8바이트 고정 헤더: `[type: 3바이트][chunk: 1바이트][size: u32 LE]`

```rust
pub struct MessageHeader {
    pub message_type: MessageType,
    pub chunk_type: ChunkType,
    pub message_size: u32,
}
```

#### Hello / Acknowledge 핸드셰이크

```rust
pub struct HelloMessage {
    pub protocol_version: u32,    // 항상 0
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

`AcknowledgeMessage::from_hello()` 메서드는 클라이언트의 Hello 메시지와 서버 버퍼 크기를 협상하여 ACK를 생성합니다.

기본값:

| 상수 | 값 | 설명 |
|------|-----|------|
| `PROTOCOL_VERSION` | 0 | OPC UA 프로토콜 버전 |
| `DEFAULT_BUFFER_SIZE` | 65,535 | 64 KB |
| `DEFAULT_MAX_MESSAGE_SIZE` | 16,777,216 | 16 MB |
| `DEFAULT_MAX_CHUNK_COUNT` | 5,000 | 최대 청크 수 |

### Transport Codec (`codec.rs`)

`tokio_util::codec`의 `Encoder`/`Decoder` 구현으로 TCP 스트림에서 메시지를 프레이밍합니다.

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

- `Decoder`: 8바이트 헤더를 먼저 파싱 → `message_size` 확인 → 전체 메시지 수신 → `RawMessage` 반환
- `Encoder`: 헤더 + 본문을 연결하여 전송

### TCP Listener (`tcp_listener.rs`)

```rust
pub struct TcpTransportConfig {
    pub bind_address: SocketAddr,
    pub max_connections: usize,          // 기본 1000
    pub connection_timeout: Duration,    // 기본 60초
    pub server_buffer_size: u32,         // 기본 65535
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

주요 메서드:

| 메서드 | 설명 |
|--------|------|
| `new(config, registry, context)` | 리스너 생성 |
| `run()` | TCP accept 루프 실행 (blocking) |
| `shutdown()` | 종료 시그널 전송 |
| `metrics()` | 트랜스포트 메트릭 참조 |

연결 수락 시 `max_connections`를 초과하면 연결을 거부하고 `metrics.record_rejection()`을 기록합니다.

### Connection Handler (`connection.rs`)

개별 TCP 연결의 전체 OPC UA 라이프사이클을 처리합니다.

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

연결 처리 흐름:

```text
1. HEL (Hello) 수신 → ACK (Acknowledge) 응답
2. OPN (OpenSecureChannel) 수신 → SecureChannel 생성 → OPN 응답
3. MSG (Message) 수신/응답 루프:
   ├── SequenceHeader + Payload 파싱
   ├── ServiceRegistry.dispatch() → 핸들러 호출
   └── 응답 인코딩 → MSG 전송
4. CLO (CloseSecureChannel) 수신 → 연결 종료
```

내부 헬퍼:

| 함수 | 용도 |
|------|------|
| `build_opn_response()` | OpenSecureChannelResponse 인코딩 |
| `build_service_fault()` | ServiceFault 응답 생성 |
| `encode_error()` | 에러 메시지 인코딩 |

### Transport Metrics (`metrics.rs`)

모든 필드가 `AtomicU64`로 lock-free 카운터입니다.

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

## Channel 모듈

> `crates/mabi-opcua/src/channel/`
>
> OPC UA Secure Channel Layer

### SecureChannel (`secure_channel.rs`)

보안 채널 생성, 토큰 발행, 시퀀스 번호 관리를 담당합니다. `SecurityPolicy::None`인 경우 실제 암호화는 수행하지 않습니다.

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

채널 ID와 토큰 ID는 전역 `AtomicU32` 카운터로 고유하게 할당됩니다.

| 메서드 | 설명 |
|--------|------|
| `new_unsecured()` | SecurityPolicy::None 채널 생성 |
| `new(policy, mode, lifetime)` | 지정된 보안 설정으로 생성 |
| `next_server_sequence_number()` | 서버 시퀀스 번호 원자적 증가 |
| `validate_sequence_number(received)` | 클라이언트 시퀀스 번호 검증 |
| `renew_token(lifetime)` | 보안 토큰 갱신 |
| `is_token_expired()` | 토큰 만료 여부 확인 |

### 메시지 보안 헤더 (`message.rs`)

OPN/MSG/CLO 메시지의 보안 헤더 및 시퀀스 헤더를 처리합니다.

```rust
/// OPN 메시지의 비대칭 보안 헤더
pub struct AsymmetricSecurityHeader {
    pub security_policy_uri: String,
    pub sender_certificate: Vec<u8>,
    pub receiver_certificate_thumbprint: Vec<u8>,
}

/// MSG 메시지의 대칭 보안 헤더
pub struct SymmetricSecurityHeader {
    pub token_id: u32,
}

/// 시퀀스 헤더 (모든 보안 메시지 공통)
pub struct SequenceHeader {
    pub sequence_number: u32,
    pub request_id: u32,
}
```

OPN/MSG 메시지 바디 파싱 및 응답 빌드 함수:

```rust
pub struct OpenSecureChannelBody { ... }
pub struct SecureMessageBody { ... }

pub fn build_opn_response_body(channel_id, seq_header, payload) -> Vec<u8>;
pub fn build_msg_response_body(channel_id, token_id, seq_header, payload) -> Vec<u8>;
```

---

## Service 모듈

> `crates/mabi-opcua/src/service/`
>
> OPC UA Part 4 기반 서비스 핸들러 계층

### 서비스 레지스트리 (`registry.rs`)

요청 타입 NodeId를 키로 핸들러를 등록하고 디스패치합니다.

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

### 서비스 핸들러 목록

모든 핸들러는 `ServiceHandler` 트레이트를 구현하며, 각 모듈의 `register_handlers()` 함수로 레지스트리에 일괄 등록됩니다.

#### Discovery (`discovery.rs`) — Part 4, Section 5.4

| 핸들러 | Request ID | Response ID |
|--------|-----------|-------------|
| `GetEndpointsHandler` | 428 | 431 |

공통 구조체:
- `RequestHeader` — 모든 요청의 공통 헤더 (authentication_token, timestamp, request_handle, timeout_hint 등)
- `ResponseHeader` — 응답 헤더 (timestamp, request_handle, service_result)
- `encode_application_description()` — 서버 ApplicationDescription 인코딩

#### Session (`session.rs`) — Part 4, Section 5.6

| 핸들러 | Request ID | Response ID |
|--------|-----------|-------------|
| `CreateSessionHandler` | 461 | 464 |
| `ActivateSessionHandler` | 467 | 470 |
| `CloseSessionHandler` | 473 | 476 |

#### Attribute (`attribute.rs`) — Part 4, Section 5.10

| 핸들러 | Request ID | Response ID |
|--------|-----------|-------------|
| `ReadHandler` | 631 | 634 |
| `WriteHandler` | 673 | 676 |

#### Browse (`browse.rs`) — Part 4, Section 5.8

| 핸들러 | Request ID | Response ID |
|--------|-----------|-------------|
| `BrowseHandler` | 527 | 530 |

#### Subscription (`subscription.rs`) — Part 4, Section 5.13

| 핸들러 | Request ID | Response ID |
|--------|-----------|-------------|
| `CreateSubscriptionHandler` | 787 | 790 |
| `DeleteSubscriptionsHandler` | 847 | 850 |
| `PublishHandler` | 826 | 829 |

#### MonitoredItem (`monitored_item.rs`) — Part 4, Section 5.12

| 핸들러 | Request ID | Response ID |
|--------|-----------|-------------|
| `CreateMonitoredItemsHandler` | 751 | 754 |
| `DeleteMonitoredItemsHandler` | 781 | 784 |

### 등록 흐름

`OpcUaServer::create_tcp_listener()` 에서 모든 핸들러를 등록합니다:

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

## 서버 통합

기존 `OpcUaServer::start()` 메서드에 TCP 리스너가 통합되었습니다:

```rust
pub async fn start(&self) -> OpcUaResult<()> {
    // ... 기존 초기화 ...

    // TCP 리스너를 백그라운드 태스크로 실행
    let tcp_listener = self.create_tcp_listener()?;
    tokio::spawn(async move {
        if let Err(e) = tcp_listener.run().await {
            warn!(error = %e, "TCP listener error");
        }
    });

    // ...
}
```

`create_tcp_listener()` 내부에서:
1. `endpoint_url` (`opc.tcp://0.0.0.0:4840`)을 `SocketAddr`로 파싱
2. 서비스 핸들러 등록
3. `ServiceContextTemplate` (서버 컴포넌트 Arc 참조) 생성
4. `OpcUaTcpListener` 구성

---

## 에러 타입 추가

`OpcUaError`에 다음 variant가 추가되었습니다:

| Variant | 설명 |
|---------|------|
| `Codec(String)` | 바이너리 인코딩/디코딩 에러 |
| `ProtocolError(String)` | 프로토콜 수준 에러 |
| `ServiceNotSupported { service_id }` | 미지원 서비스 요청 |
| `BadSecureChannelId(u32)` | 잘못된 Secure Channel ID |
| `BadSequenceNumber { expected, actual }` | 시퀀스 번호 불일치 |
| `MessageTooLarge { size, max }` | 최대 메시지 크기 초과 |
| `Bind { address, reason }` | TCP 바인드 실패 |

---

## 설계 패턴

| 패턴 | 적용 위치 |
|------|----------|
| **트레이트 기반 직렬화** | `BinaryEncodable` / `BinaryDecodable` |
| **Async 서비스 핸들러** | `ServiceHandler` (async_trait) |
| **HashMap 디스패치** | `ServiceRegistry` — NodeId → Handler |
| **원자적 카운터** | SecureChannel ID/Token ID, TransportMetrics |
| **컨텍스트 템플릿** | `ServiceContextTemplate` → 연결별 `ServiceContext` |
| **tokio-util 코덱** | `OpcUaTransportCodec` — Encoder/Decoder |

---

## 데이터 흐름

```text
OPC UA Client
     │
     ▼
┌─────────────────┐
│  TCP Connection  │  ← tokio::net::TcpStream
└────────┬────────┘
         ▼
┌─────────────────┐
│  Transport Codec │  ← OpcUaTransportCodec (프레이밍)
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
│  Secure Channel │  ← 시퀀스 번호 검증, 토큰 관리
│  + Message      │
│    Headers      │
└────────┬────────┘
         ▼
┌─────────────────┐
│  Service        │  ← ServiceRegistry.dispatch()
│  Registry       │     ExtensionObject → type_id → Handler
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
