# mabi-opcua Secure Channel Token Renewal 구현

> 2026-02 TRAP OPC UA 장시간 운영 테스트에서 발견된 secure channel 갱신 미구현 수정

---

## 증상

TRAP Gateway가 mabi OPC UA 시뮬레이터에 연결 후 약 45초 경과 시점부터 모든 서비스 요청이 타임아웃:

```
opcua::client::session::session_state: Making secure channel request
opcua::client::session::session_state: security_mode = None
opcua::client::session::session_state: Timeout waiting for response from server
opcua::client::message_queue: Request 103 has timed out and any response will be ignored
```

이후 10초 간격으로 동일 패턴이 무한 반복되며, 클라이언트 측 데이터 수신이 완전히 중단됨.

---

## 근본 원인

### OPC UA Secure Channel 갱신 프로토콜 (Part 6 Section 6.7.4)

OPC UA 클라이언트는 토큰 만료 전에 `OpenSecureChannel` 요청을 `requestType=1` (Renew)로 전송하여 보안 토큰을 갱신한다. 서버는 **기존 `channel_id`를 유지**하고 **새 `token_id`만 발급**해야 한다.

### 기존 코드의 문제점

`transport/connection.rs`에서:

1. OPN 메시지 처리가 Phase 2 (초기 연결)에서만 수행되고, Phase 3 (서비스 루프)에서는 OPN을 처리하지 않음
2. `requestType` 필드를 `_request_type`로 디코딩만 하고 무시
3. 매번 `SecureChannel::new_unsecured()`로 새 채널 생성

`channel/secure_channel.rs`에서:

4. `renew_token(&mut self)` — `Arc<SecureChannel>` 환경에서 호출 불가
5. `token_id`, `token_lifetime_ms` 필드가 immutable

### 실패 시나리오

```
Client → OPN (requestType=Renew, channel_id=1) → Server
Server: Phase 3 message loop에서 OPN 메시지 타입 미처리
        → "Unexpected message type" 경고 후 무시
Client: 응답 없음 → 10초 타임아웃 → 재시도 → 무한 반복
```

---

## 수정 내용

### 1. `SecureChannel` Interior Mutability 전환

토큰 관련 필드를 atomic/lock 기반으로 변경하여 `Arc<SecureChannel>` 환경에서 갱신 가능:

| 필드 | Before | After |
|------|--------|-------|
| `token_id` | `u32` | `AtomicU32` |
| `token_lifetime_ms` | `u32` | `AtomicU32` |
| `token_created_at` | `Instant` | `RwLock<Instant>` |
| `renew_token()` | `&mut self` | `&self` |

### 2. Phase 3 메시지 루프에 OPN 갱신 처리 추가

서비스 루프에서 `MessageType::OpenSecureChannel` 수신 시:
- 기존 채널의 `channel_id` 유지
- `channel.renew_token(lifetime)` 호출로 새 `token_id` 발급
- `OpenSecureChannelResponse` 응답 전송

### 3. OPN 디코딩 로직 헬퍼 추출

Phase 2 (Issue)와 Phase 3 (Renew)에서 공유하는 `decode_opn_request_fields()` 함수 추출.

---

## 수정 후 동작

```
Client → OPN (requestType=Issue)  → Server: 새 channel_id=1, token_id=1 발급
         ... 토큰 만료 전 ...
Client → OPN (requestType=Renew)  → Server: channel_id=1 유지, token_id=2 발급
Client → MSG (token_id=2)         → Server: 정상 처리
         ... 반복 ...
```

---

## 영향 범위

- `crates/mabi-opcua/src/channel/secure_channel.rs` — Interior mutability 전환
- `crates/mabi-opcua/src/transport/connection.rs` — OPN 갱신 처리 추가, 디코딩 헬퍼 추출
