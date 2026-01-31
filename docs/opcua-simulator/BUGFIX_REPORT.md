# mabi-opcua TCP Transport — OPC UA 호환성 버그 수정 보고서

> 2026-01 TRAP OPC UA 통합 테스트에서 발견된 2건의 버그 수정 보고서

---

## 테스트 환경

- **시뮬레이터**: mabi-cli v1.1.0 (`mabi opcua --port 4840 --nodes 100`)
- **클라이언트**: TRAP Gateway (`trap-opcua` 크레이트, `opcua-rs` 라이브러리 기반)
- **테스트 범위**: 기본 연결, 100태그 대량 폴링, 100ms 고빈도 폴링, 다중 디바이스 병렬, 재연결 복원력
- **결과**: 2건의 프로토콜 호환성 버그 발견 및 수정 후 전체 테스트 통과

---

## Bug 1: 서비스 디스패치 메시지 형식 불일치 (Critical)

### 증상

TRAP 클라이언트가 `OpenSecureChannel` 이후 첫 서비스 요청(`GetEndpoints`, `i=428`)을 보내면 mabi 서버에서 다음 에러 발생:

```
Service handler error: Codec error: Empty buffer for NodeId
```

클라이언트 측에서는 `ServiceFault` 응답을 받고 세션 생성에 실패:

```
Array length is negative value and invalid
```

### 근본 원인

`service/registry.rs`의 `dispatch()` 메서드가 서비스 요청 페이로드를 **ExtensionObject** 형식(`NodeId + encoding_byte + length + body`)으로 디코딩했으나, OPC UA Part 6, Section 6.7.3에 따르면 MSG 메시지 내부의 서비스 요청은 **raw `NodeId + body`** 형식으로 인코딩됩니다.

```text
실제 페이로드:        [NodeId][RequestHeader + body...]
dispatch()가 기대:    [NodeId][encoding_byte (0x01)][i32 length][body...]
                              ↑ RequestHeader의 첫 바이트를 인코딩 마스크로 오해석
```

이로 인해:
1. `ExtensionObject::decode()`가 body의 첫 바이트를 `encoding_byte`로 읽음
2. 다음 4바이트를 body 길이로 읽어 잘못된 크기의 body를 추출
3. 핸들러에 전달된 `request_body`가 완전히 어긋남
4. 핸들러에서 `NodeId::decode()` 실패 ("Empty buffer for NodeId")
5. `ServiceFault` 응답도 ExtensionObject로 래핑되어 클라이언트가 파싱 실패

### 수정

**`service/registry.rs` — `dispatch()` 메서드:**

```rust
// Before (잘못된 ExtensionObject 디코딩)
let ext_obj = ExtensionObject::decode(&mut buf)?;
let type_id = &ext_obj.type_id;
let request_body = ext_obj.body.as_deref().unwrap_or(&[]);
// ... response도 ExtensionObject로 래핑 ...

// After (OPC UA 표준 준수 — raw NodeId + body)
let type_id = NodeId::decode(&mut buf)?;
let request_body = buf.as_ref();  // NodeId 이후 나머지 전체가 body
// ... response도 NodeId + body로 직접 연결 ...
```

**`transport/connection.rs` — `build_service_fault()` 함수:**

```rust
// Before (ExtensionObject 래핑)
let ext = ExtensionObject {
    type_id: NodeId::numeric(0, 397),
    body: Some(response_header_bytes),
};
ext.encode(&mut buf)?;

// After (raw NodeId + body)
NodeId::numeric(0, 397).encode(&mut buf)?;
response_header.encode(&mut buf)?;
```

### 영향 범위

- 모든 OPC UA 서비스 요청/응답에 영향
- ExtensionObject 래핑 없이 직접 NodeId + body 인코딩으로 변경
- OPC UA Part 6 표준과 `opcua-rs` 클라이언트 라이브러리 모두와 호환

---

## Bug 2: CreateSession 요청 파싱 시 LocalizedText 디코딩 오류 (Critical)

### 증상

Bug 1 수정 후, `GetEndpoints` 요청은 성공하지만 `CreateSession` (`i=461`) 요청에서 다음 에러 발생:

```
Service handler error: Codec error: Not enough data: need 4610 bytes, have 108
```

### 근본 원인

`service/session.rs`의 `CreateSessionHandler`에서 `CreateSessionRequest`의 `ClientDescription` (ApplicationDescription) 필드를 파싱할 때, `ApplicationName` 필드를 `LocalizedText`가 아닌 `String` 2개로 읽고 있었습니다.

```rust
// Before (잘못된 파싱)
let _app_name_locale = String::decode(&mut buf)?;  // LocalizedText를 String으로
let _app_name_text = String::decode(&mut buf)?;     // 바이트 오프셋 어긋남
// 이후 필드들 (ApplicationType, GatewayServerUri 등) 전혀 파싱하지 않음
```

OPC UA 표준에 따르면 `LocalizedText`는 `encoding_mask (u8) + locale (String) + text (String)` 형식이므로, 마스크 바이트 1개가 누락되어 이후 모든 필드의 바이트 오프셋이 어긋납니다. 또한 `ApplicationDescription`의 나머지 필드들(`ApplicationType`, `GatewayServerUri`, `DiscoveryProfileUri`, `DiscoveryUrls` 배열)도 파싱하지 않아 이후 `ServerUri`, `EndpointUrl`, `SessionName`, `ClientNonce` 등의 필드가 잘못된 위치에서 읽히게 됩니다.

잘못된 바이트를 `String::decode()`의 길이 프리픽스로 해석하면서 `4610 bytes` 같은 비정상적인 길이 요구가 발생합니다.

### 수정

**`service/session.rs` — `CreateSessionHandler::handle()`:**

```rust
// After (OPC UA Part 4, Section 5.6.2 기준 전체 필드 파싱)

// ClientDescription (ApplicationDescription)
let _app_uri = String::decode(&mut buf)?;
let _product_uri = String::decode(&mut buf)?;
let _app_name = LocalizedText::decode(&mut buf)?;    // LocalizedText로 올바르게 디코딩
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

### 영향 범위

- `CreateSession` 서비스 핸들러의 요청 파싱에만 영향
- 수정 후 전체 세션 라이프사이클 (CreateSession → ActivateSession → Read/Write → CloseSession) 정상 동작 확인

---

## 수정 후 검증 결과

### 테스트 시나리오 및 결과

| # | 시나리오 | 설정 | 결과 | 상세 |
|---|---------|------|------|------|
| 1 | 기본 연결 | 5태그, 1초 폴링 | **PASS** | OPN→GetEndpoints→CreateSession→ActivateSession→Read 전체 흐름 정상 |
| 2 | 대량 태그 | 100태그, 1초 폴링 | **PASS** | 63 reads/sec, 메모리 20MB, 데이터 에러 0 |
| 3 | 고빈도 폴링 | 10태그, 100ms 폴링 | **PASS** | 100 reads/sec (기대치 정확 일치), 메모리 안정 |
| 4 | 다중 디바이스 | 3디바이스 (2 정상 + 1 불능) | **PASS** | 정상 디바이스 독립 동작, 실패 디바이스 자동 재연결 |
| 5 | Graceful Degradation | 불가용 디바이스 포함 | **PASS** | Circuit breaker Closed, 10초 주기 재연결, 타 디바이스 무영향 |

### 성능 측정

| 메트릭 | 측정값 | 비고 |
|--------|--------|------|
| Read 처리량 (100태그, 1초 폴링) | ~63 reads/sec | 태그별 개별 읽기 |
| Read 처리량 (10태그, 100ms 폴링) | ~100 reads/sec | 기대치 정확 일치 |
| 메모리 사용량 | ~20MB RSS | 100태그 폴링 시 |
| OPC UA 세션 수립 시간 | ~1초 이내 | GetEndpoints → CreateSession → ActivateSession |
| 자동 재연결 주기 | 10초 | DriverManager::spawn_reconnect_monitor() |

---

## 수정 파일 요약

| 파일 | 변경 유형 | 설명 |
|------|----------|------|
| `crates/mabi-opcua/src/service/registry.rs` | **버그 수정** | `dispatch()` 디코딩을 ExtensionObject에서 raw NodeId+body로 변경 |
| `crates/mabi-opcua/src/service/session.rs` | **버그 수정** | `CreateSessionHandler` 요청 파싱에서 LocalizedText 및 ApplicationDescription 전체 필드 올바르게 디코딩 |
| `crates/mabi-opcua/src/transport/connection.rs` | **버그 수정** | `build_service_fault()` 응답 인코딩을 ExtensionObject에서 raw NodeId+body로 변경 |

---

## 교훈

1. **OPC UA 바이너리 프로토콜의 메시지 래핑 구분**: MSG 최상위 서비스 페이로드는 `NodeId + body`로 직접 인코딩되며, `ExtensionObject` 형식은 `AdditionalHeader` 같은 내부 필드에만 사용됨 (Part 6, Section 6.7.3)
2. **LocalizedText vs String**: OPC UA의 `LocalizedText`는 `encoding_mask` 바이트가 선행하므로 단순 `String` 2개로 대체할 수 없음. 1바이트 차이가 이후 전체 필드 파싱을 무너뜨림
3. **부분 파싱의 위험성**: 요청의 일부 필드만 파싱하면 가변 길이 필드(String, ByteString, Array 등)의 오프셋이 어긋나 후속 필드에서 예측 불가능한 에러 발생. 전체 필드를 순서대로 읽어야 함
