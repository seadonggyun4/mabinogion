# mabi-knx Bugfix Report

> **Date**: February 2026
>
> **Context**: Bugs discovered and fixed during integration testing with the TRAP KNX driver

---

## Bug #1: --groups Parameter Does Not Create Group Objects

### Symptoms
- Running `mabi knx --groups 200` displayed "Group Objects: 200" but no actual group objects were created
- GroupValueRead/Write requests received empty responses

### Root Cause
`KnxCommand::start_server()` only passed the `--groups` value to config without actually creating objects in the `GroupObjectTable`.

### Fix
Added group object creation logic to `start_server()`:

```rust
let group_table = Arc::new(GroupObjectTable::new());
let dpt_types = [
    DptId::new(1, 1),   // Switch (bool)
    DptId::new(5, 1),   // Scaling (0-100%)
    DptId::new(9, 1),   // Temperature (float16)
    DptId::new(9, 4),   // Lux
    DptId::new(9, 7),   // Humidity
    DptId::new(12, 1),  // Counter (u32)
    DptId::new(13, 1),  // Counter signed (i32)
    DptId::new(14, 56), // Float (f32)
];
for i in 0..self.group_objects {
    let main = ((i / 256) + 1) as u8;
    let middle = ((i / 8) % 8) as u8;
    let sub = (i % 256) as u8;
    let addr = GroupAddress::three_level(main, middle, sub);
    let dpt_idx = i % dpt_types.len();
    group_table.create(addr, &name, &dpt_types[dpt_idx])?;
}
let server = Arc::new(KnxServer::new(config).with_group_objects(group_table));
```

### Affected Files
- `crates/mabi-cli/src/commands/protocol.rs`

---

## Bug #2: GroupValueRead Response Not Implemented

### Symptoms
- Client sent GroupValueRead but only received ACK, no GroupValueResponse
- Read operations always timed out

### Root Cause
In `server.rs`'s `process_cemi()`, the `Apci::GroupValueRead` case only had a `// TODO: Send response via tunnelling` comment.

### Fix
Implemented GroupValueResponse sending logic:

```rust
Apci::GroupValueRead => {
    let response_data = match self.group_objects.read(&group_addr) {
        Ok(data) => data,
        Err(_) => vec![0u8],
    };
    let response_cemi = CemiFrame::group_value_response(
        self.config.individual_address, group_addr, response_data,
    );
    let seq = connection.next_send_sequence();
    let tunnel_req = TunnellingRequest::new(connection.channel_id, seq, response_cemi);
    let frame = KnxFrame::new(ServiceType::TunnellingRequest, tunnel_req.encode());
    socket.send_to(&frame.encode(), client_addr).await?;
}
```

### Affected Files
- `crates/mabi-knx/src/server.rs`

---

## Bug #3: CemiFrame encode/decode — APCI Byte Order and npdu_len Error

### Symptoms
- Server silently failed to parse TRAP client's TunnellingRequest
- CemiFrame::decode() returned "frame too short" error

### Root Cause (2 issues)

#### 3a. npdu_len Byte Count Error

Per the KNX standard, `npdu_len` is a count that includes the TPCI/APCI byte.
That is, exactly `npdu_len` bytes follow after the npdu_len field.

```rust
// Before fix (decode)
if buf.len() < npdu_len + 1 { ... }  // Required npdu_len + 1 bytes
let apci_high = buf.get_u8();
let apci_low = buf.get_u8();         // Always read 2 bytes

// After fix
if buf.len() < npdu_len { ... }       // Only npdu_len bytes needed
let apci_byte1 = buf.get_u8();        // Read only 1 byte (APCI)
```

```rust
// Before fix (encode, full data path)
buf.put_u8((self.data.len() + 1) as u8);  // npdu_len
buf.put_u8((apci >> 8) as u8);            // APCI high
buf.put_u8(apci as u8);                    // APCI low
buf.put_slice(&self.data);                 // data
// Total: npdu_len + 1 bytes output (mismatch)

// After fix
buf.put_u8((self.data.len() + 1) as u8);  // npdu_len
buf.put_u8(apci_byte);                     // APCI 1 byte
buf.put_slice(&self.data);                 // data
// Total: npdu_len bytes output (correct)
```

#### 3b. APCI Byte Mapping Error

Mabinogion's internal APCI representation (0x0000=Read, 0x0040=Response, 0x0080=Write) maps directly to the first NPDU byte values in KNX wire format. However, the previous code used `apci >> 8` as the first byte and `apci & 0xFF` as the second byte.

```rust
// Before fix (encode)
buf.put_u8((apci >> 8) as u8);   // GroupValueWrite: 0x0080 >> 8 = 0x00 (wrong!)
buf.put_u8(apci as u8);          // 0x80

// After fix
let apci_byte = apci as u8;       // GroupValueWrite: 0x0080 as u8 = 0x80 (correct!)
buf.put_u8(apci_byte);
```

```rust
// Before fix (decode)
let apci_raw = ((byte1 as u16) << 8) | (byte2 as u16);
// TRAP sends [0x80, 0x01] → raw = 0x8001 → Unknown! (out of range)

// After fix
let apci_raw = byte1 as u16;
// TRAP sends [0x80, 0x01] → raw = 0x80 → GroupValueWrite ✓
```

### Affected Files
- `crates/mabi-knx/src/cemi.rs` (encode + decode)

---

## Post-Fix Verification

### Direct Python Test
```bash
python3 /tmp/knx_test.py
# Connected: channel_id=28
# Frame 1: svc=0x0421 (ACK)
# Frame 2: svc=0x0420 (TunnellingRequest with GroupValueResponse)
```

### TRAP Integration Tests: 25/25 PASS

```bash
cargo test --test knx_mabi_integration_test -- --test-threads=1
# test result: ok. 25 passed; 0 failed; 0 ignored
```

| Verification Item | Result |
|-------------------|--------|
| 200 group objects created | PASS |
| GroupValueRead → GroupValueResponse | PASS |
| GroupValueWrite | PASS |
| cEMI encode/decode round-trip | PASS (7 unit tests) |
| Multiple concurrent client connections | PASS |
| Sustained polling (30 cycles) | PASS, 100% |

---

## Reference: KNX cEMI NPDU Wire Format Summary

```
After npdu_len field, exactly npdu_len bytes follow:

npdu_len=1 (small data):
  [TPCI/APCI byte]
  - Bits 7-6: APCI (00=Read, 01=Response, 10=Write)
  - Bits 5-0: small data (up to 6 bits)

npdu_len≥2 (full data):
  [TPCI/APCI byte] [data byte 1] [data byte 2] ...
  - Byte 0: APCI in bits 7-6
  - Bytes 1+: payload data (npdu_len - 1 bytes)
```
