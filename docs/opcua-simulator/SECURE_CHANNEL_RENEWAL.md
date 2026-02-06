# mabi-opcua Secure Channel Token Renewal Implementation

> Fix for missing secure channel renewal, discovered during prolonged TRAP OPC UA operation testing in 2026-02

---

## Symptoms

Approximately 45 seconds after the TRAP Gateway establishes a connection to the mabi OPC UA simulator, all service requests begin timing out:

```
opcua::client::session::session_state: Making secure channel request
opcua::client::session::session_state: security_mode = None
opcua::client::session::session_state: Timeout waiting for response from server
opcua::client::message_queue: Request 103 has timed out and any response will be ignored
```

This pattern then repeats indefinitely at 10-second intervals, resulting in a complete cessation of data reception on the client side.

---

## Root Cause

### OPC UA Secure Channel Renewal Protocol (Part 6, Section 6.7.4)

An OPC UA client must renew its security token prior to expiration by sending an `OpenSecureChannel` request with `requestType=1` (Renew). The server is required to **retain the existing `channel_id`** and **issue only a new `token_id`**.

### Defects in the Existing Implementation

In `transport/connection.rs`:

1. OPN message handling was only performed during Phase 2 (initial connection establishment); OPN messages were not processed during Phase 3 (service message loop)
2. The `requestType` field was decoded as `_request_type` and subsequently ignored
3. A new channel was instantiated via `SecureChannel::new_unsecured()` on every invocation

In `channel/secure_channel.rs`:

4. `renew_token(&mut self)` was not callable in an `Arc<SecureChannel>` context
5. The `token_id` and `token_lifetime_ms` fields were immutable

### Failure Scenario

```
Client -> OPN (requestType=Renew, channel_id=1) -> Server
Server: OPN message type not handled in Phase 3 message loop
        -> "Unexpected message type" warning emitted, message discarded
Client: No response received -> 10-second timeout -> retry -> infinite loop
```

---

## Changes

### 1. Transition `SecureChannel` to Interior Mutability

Token-related fields were converted to atomic/lock-based types to enable renewal through an `Arc<SecureChannel>`:

| Field | Before | After |
|-------|--------|-------|
| `token_id` | `u32` | `AtomicU32` |
| `token_lifetime_ms` | `u32` | `AtomicU32` |
| `token_created_at` | `Instant` | `RwLock<Instant>` |
| `renew_token()` | `&mut self` | `&self` |

### 2. Add OPN Renewal Handling to the Phase 3 Message Loop

Upon receiving a `MessageType::OpenSecureChannel` in the service message loop:
- Retain the existing channel's `channel_id`
- Invoke `channel.renew_token(lifetime)` to issue a new `token_id`
- Send an `OpenSecureChannelResponse` back to the client

### 3. Extract OPN Decoding Logic into a Shared Helper

A `decode_opn_request_fields()` function was extracted and shared between Phase 2 (Issue) and Phase 3 (Renew) processing paths.

---

## Post-Fix Behavior

```
Client -> OPN (requestType=Issue)  -> Server: issues new channel_id=1, token_id=1
          ... before token expiration ...
Client -> OPN (requestType=Renew)  -> Server: retains channel_id=1, issues token_id=2
Client -> MSG (token_id=2)         -> Server: processes normally
          ... cycle repeats ...
```

---

## Affected Files

- `crates/mabi-opcua/src/channel/secure_channel.rs` -- Interior mutability transition
- `crates/mabi-opcua/src/transport/connection.rs` -- OPN renewal handling, decoding helper extraction
