# mabi-opcua TCP Transport — OPC UA Compatibility Bugfix Report

> Report on two bugs discovered during TRAP OPC UA integration testing, January 2026

---

## Test Environment

- **Simulator**: mabi-cli v1.1.0 (`mabi opcua --port 4840 --nodes 100`)
- **Client**: TRAP Gateway (`trap-opcua` crate, based on the `opcua-rs` library)
- **Test Scope**: Basic connectivity, bulk polling of 100 tags, high-frequency polling at 100 ms intervals, parallel multi-device operation, reconnection resilience
- **Result**: Two protocol-level compatibility bugs were identified and resolved; all tests passed after the fix

---

## Bug 1: Service Dispatch Message Format Mismatch (Critical)

### Symptoms

When the TRAP client sent its first service request (`GetEndpoints`, `i=428`) after completing the `OpenSecureChannel` handshake, the mabi server raised the following error:

```
Service handler error: Codec error: Empty buffer for NodeId
```

On the client side, a `ServiceFault` response was received, causing session establishment to fail:

```
Array length is negative value and invalid
```

### Root Cause

The `dispatch()` method in `service/registry.rs` was decoding the service request payload in **ExtensionObject** format (`NodeId + encoding_byte + length + body`). However, according to OPC UA Part 6, Section 6.7.3, the service request within an MSG message is encoded in **raw `NodeId + body`** format.

```text
Actual payload:           [NodeId][RequestHeader + body...]
dispatch() expected:      [NodeId][encoding_byte (0x01)][i32 length][body...]
                                  ^ First byte of RequestHeader misinterpreted as encoding mask
```

This caused the following cascade of failures:
1. `ExtensionObject::decode()` read the first byte of the body as the `encoding_byte`
2. The next 4 bytes were interpreted as the body length, extracting an incorrectly sized body
3. The `request_body` passed to the handler was completely misaligned
4. The handler failed at `NodeId::decode()` ("Empty buffer for NodeId")
5. The `ServiceFault` response was also wrapped in an ExtensionObject, causing the client to fail parsing it

### Fix

**`service/registry.rs` -- `dispatch()` method:**

```rust
// Before (incorrect ExtensionObject decoding)
let ext_obj = ExtensionObject::decode(&mut buf)?;
let type_id = &ext_obj.type_id;
let request_body = ext_obj.body.as_deref().unwrap_or(&[]);
// ... response was also wrapped in ExtensionObject ...

// After (OPC UA specification-compliant raw NodeId + body)
let type_id = NodeId::decode(&mut buf)?;
let request_body = buf.as_ref();  // Entire remainder after NodeId constitutes the body
// ... response is also directly concatenated as NodeId + body ...
```

**`transport/connection.rs` -- `build_service_fault()` function:**

```rust
// Before (ExtensionObject wrapping)
let ext = ExtensionObject {
    type_id: NodeId::numeric(0, 397),
    body: Some(response_header_bytes),
};
ext.encode(&mut buf)?;

// After (raw NodeId + body)
NodeId::numeric(0, 397).encode(&mut buf)?;
response_header.encode(&mut buf)?;
```

### Impact Scope

- Affected all OPC UA service request/response processing
- Changed from ExtensionObject wrapping to direct NodeId + body encoding
- Now compatible with both the OPC UA Part 6 specification and the `opcua-rs` client library

---

## Bug 2: LocalizedText Decoding Error During CreateSession Request Parsing (Critical)

### Symptoms

After Bug 1 was resolved, `GetEndpoints` requests succeeded, but `CreateSession` (`i=461`) requests failed with the following error:

```
Service handler error: Codec error: Not enough data: need 4610 bytes, have 108
```

### Root Cause

In `service/session.rs`, the `CreateSessionHandler` was parsing the `ClientDescription` (`ApplicationDescription`) field of the `CreateSessionRequest` by reading the `ApplicationName` field as two separate `String` values instead of a single `LocalizedText`.

```rust
// Before (incorrect parsing)
let _app_name_locale = String::decode(&mut buf)?;  // LocalizedText read as String
let _app_name_text = String::decode(&mut buf)?;     // Byte offset misalignment begins here
// Remaining fields (ApplicationType, GatewayServerUri, etc.) were not parsed at all
```

Per the OPC UA specification, `LocalizedText` is encoded as `encoding_mask (u8) + locale (String) + text (String)`. Omitting the 1-byte mask caused every subsequent field to be read at an incorrect byte offset. Furthermore, the remaining fields of `ApplicationDescription` (`ApplicationType`, `GatewayServerUri`, `DiscoveryProfileUri`, and the `DiscoveryUrls` array) were not parsed at all, causing downstream fields such as `ServerUri`, `EndpointUrl`, `SessionName`, and `ClientNonce` to be read from incorrect positions.

Interpreting misaligned bytes as the length prefix for `String::decode()` produced nonsensical length values such as `4610 bytes`.

### Fix

**`service/session.rs` -- `CreateSessionHandler::handle()`:**

```rust
// After (complete field parsing per OPC UA Part 4, Section 5.6.2)

// ClientDescription (ApplicationDescription)
let _app_uri = String::decode(&mut buf)?;
let _product_uri = String::decode(&mut buf)?;
let _app_name = LocalizedText::decode(&mut buf)?;    // Correctly decoded as LocalizedText
let _app_type = u32::decode(&mut buf)?;               // ApplicationType
let _gateway_uri = String::decode(&mut buf)?;         // GatewayServerUri
let _discovery_uri = String::decode(&mut buf)?;       // DiscoveryProfileUri
// DiscoveryUrls array
let discovery_urls_len = i32::decode(&mut buf)?;
if discovery_urls_len > 0 {
    for _ in 0..discovery_urls_len {
        let _ = String::decode(&mut buf)?;
    }
}
// ServerUri
let _server_uri = String::decode(&mut buf)?;
// EndpointUrl
let _endpoint_url = String::decode(&mut buf)?;
// SessionName
let _session_name_req = String::decode(&mut buf)?;
// ClientNonce (ByteString)
let _client_nonce = Vec::<u8>::decode(&mut buf)?;
// ClientCertificate (ByteString)
let _client_cert = Vec::<u8>::decode(&mut buf)?;
// RequestedSessionTimeout (Double)
let _requested_timeout = f64::decode(&mut buf)?;
// MaxResponseMessageSize (UInt32)
let _max_response_size = u32::decode(&mut buf)?;
```

### Impact Scope

- Affected only the request parsing logic of the `CreateSession` service handler
- After the fix, the complete session lifecycle (CreateSession, ActivateSession, Read/Write, CloseSession) was verified to function correctly

---

## Post-Fix Verification Results

### Test Scenarios and Outcomes

| # | Scenario | Configuration | Result | Details |
|---|----------|---------------|--------|---------|
| 1 | Basic connectivity | 5 tags, 1 s polling interval | **PASS** | Full sequence (OPN, GetEndpoints, CreateSession, ActivateSession, Read) completed successfully |
| 2 | Bulk tag polling | 100 tags, 1 s polling interval | **PASS** | 63 reads/sec, 20 MB memory usage, zero data errors |
| 3 | High-frequency polling | 10 tags, 100 ms polling interval | **PASS** | 100 reads/sec (exact match with expected throughput), stable memory |
| 4 | Multi-device parallel | 3 devices (2 healthy + 1 unavailable) | **PASS** | Healthy devices operated independently; failed device triggered automatic reconnection |
| 5 | Graceful degradation | Including an unavailable device | **PASS** | Circuit breaker remained Closed, reconnection attempted every 10 s, no impact on other devices |

### Performance Measurements

| Metric | Measured Value | Notes |
|--------|----------------|-------|
| Read throughput (100 tags, 1 s polling) | ~63 reads/sec | Individual per-tag reads |
| Read throughput (10 tags, 100 ms polling) | ~100 reads/sec | Exact match with expected value |
| Memory usage | ~20 MB RSS | During 100-tag polling |
| OPC UA session establishment time | < 1 second | GetEndpoints, CreateSession, ActivateSession |
| Automatic reconnection interval | 10 seconds | Via `DriverManager::spawn_reconnect_monitor()` |

---

## Modified Files Summary

| File | Change Type | Description |
|------|-------------|-------------|
| `crates/mabi-opcua/src/service/registry.rs` | **Bugfix** | Changed `dispatch()` decoding from ExtensionObject to raw NodeId + body |
| `crates/mabi-opcua/src/service/session.rs` | **Bugfix** | Corrected `CreateSessionHandler` request parsing to properly decode LocalizedText and all ApplicationDescription fields |
| `crates/mabi-opcua/src/transport/connection.rs` | **Bugfix** | Changed `build_service_fault()` response encoding from ExtensionObject to raw NodeId + body |

---

## Lessons Learned

1. **Message wrapping distinctions in the OPC UA binary protocol**: The top-level service payload within an MSG message is encoded directly as `NodeId + body`. The `ExtensionObject` format is reserved for internal fields such as `AdditionalHeader` (Part 6, Section 6.7.3).
2. **LocalizedText vs. String**: The OPC UA `LocalizedText` type is preceded by an `encoding_mask` byte and therefore cannot be substituted with two separate `String` values. A single byte discrepancy corrupts the parsing of all subsequent fields.
3. **Risks of partial parsing**: Parsing only a subset of request fields causes byte offset misalignment on variable-length fields (String, ByteString, Array, etc.), leading to unpredictable errors in downstream fields. All fields must be read sequentially in their entirety.
