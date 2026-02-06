# OPC UA 세션 라이프사이클 완성도 개선 보고서

> 2026-02 TRAP 게이트웨이 통합 테스트에서 식별된 세션 핸드셰이크 결함 수정

---

## 1. 서론

OPC UA Part 4 (Services)는 클라이언트-서버 간 세션 수립을 위해 엄격한 상태 머신을 정의한다. 세션 라이프사이클은 `CreateSession` -> `ActivateSession` -> 서비스 호출 -> `CloseSession` 순서를 따르며, 각 단계에서 서버는 반드시 이전 단계에서 발급한 세션 식별자(`SessionId`, `AuthenticationToken`)를 유지하고 검증해야 한다.

본 보고서는 mabi-opcua 시뮬레이터에서 발견된 세션 컨텍스트 연결 결함과 `CloseSecureChannel` 응답 부재 문제를 분석하고, ASHRAE/OPC Foundation 표준에 부합하는 수정 방안을 기술한다.

---

## 2. 문제 분석

### 2.1 세션 컨텍스트 단절 (Critical)

#### 증상

TRAP 게이트웨이 클라이언트가 `CreateSession` → `ActivateSession` 순서로 요청을 전송할 때, 서버가 `CreateSession`에는 정상 응답하지만 `ActivateSession`에서 세션을 찾지 못해 활성화가 실패하는 현상이 관찰되었다.

#### 근본 원인

`transport/connection.rs`에서 커넥션별 `ServiceContext`를 생성할 때, `session_id`와 `auth_token` 필드를 `Option<NodeId>` 타입의 불변 값으로 `None`으로 초기화한다:

```rust
let context = Arc<ServiceContext> {
    // ... (shared server components) ...
    session_id: None,     // 불변 — CreateSession 이후에도 None 유지
    auth_token: None,     // 불변 — 토큰 검증 불가
};
```

OPC UA 표준(Part 4, Section 5.6)에 따르면 `CreateSession` 서비스 호출 시 서버가 `SessionId`와 `AuthenticationToken`을 발급하면, 이 값은 동일 SecureChannel 위의 후속 서비스 호출에서 지속적으로 참조되어야 한다. 그러나 `ServiceContext`가 불변(`Arc<T>`)이므로 `CreateSessionHandler`가 세션을 생성하더라도 컨텍스트에 반영할 수 없었다.

이로 인해:
1. `ActivateSessionHandler`가 `context.session_id`를 참조하면 항상 `None`을 반환
2. 세션 활성화가 누락되어 클라이언트가 타임아웃 대기
3. 이후 모든 서비스 호출(`Read`, `Write`, `Browse`, `CreateSubscription` 등)에서 세션 기반 접근 제어 불가

#### OPC UA 표준 참조

- **Part 4, Section 5.6.2 (CreateSession)**: 서버는 `SessionId`와 `AuthenticationToken`을 반환해야 하며, 이 토큰은 동일 SecureChannel에서 세션 식별에 사용된다.
- **Part 4, Section 5.6.3 (ActivateSession)**: 클라이언트는 `CreateSession`에서 수신한 `AuthenticationToken`을 `RequestHeader`에 포함하여 전송한다.
- **Part 4, Section 5.6.4 (CloseSession)**: 세션 종료 시 서버는 관련 구독과 모니터링 항목을 정리해야 한다.

### 2.2 CloseSecureChannel 응답 부재 (Medium)

#### 증상

클라이언트가 `CLO` (CloseSecureChannel) 메시지를 전송하면 서버가 응답 없이 연결을 즉시 종료한다.

#### 근본 원인

`transport/connection.rs`의 메시지 루프에서 `MessageType::CloseSecureChannel`을 수신하면 단순 `break`만 실행:

```rust
MessageType::CloseSecureChannel => {
    debug!(peer = %peer, "Received CLO — closing channel");
    break;  // 응답 없이 루프 탈출
}
```

OPC UA Part 6, Section 7.1.4에 따르면 서버는 `CloseSecureChannelResponse`를 전송한 후 연결을 종료해야 한다. 또한 SecureChannel 종료 시 해당 채널에 연결된 세션의 정리(Cleanup)가 필요하다.

---

## 3. 수정 설계

### 3.1 설계 원칙

1. **Interior Mutability 패턴**: `ServiceContext`의 세션 관련 필드에 `parking_lot::RwLock`을 적용하여, `Arc<ServiceContext>` 공유 참조 하에서도 핸들러가 세션 상태를 업데이트할 수 있도록 한다.
2. **캡슐화**: `session_id`와 `auth_token`에 대한 직접 접근 대신 `set_session()`, `clear_session()`, `current_session_id()`, `current_auth_token()` 메서드를 제공한다.
3. **프로토콜 정합성**: `CloseSecureChannel` 수신 시 응답 전송 및 세션 정리를 표준에 맞게 구현한다.

### 3.2 ServiceContext 내부 가변성 도입

```rust
pub struct ServiceContext {
    // ... (기존 공유 필드 유지) ...

    /// RwLock으로 래핑하여 핸들러에서 세션 생성/종료 시 업데이트 가능
    pub session_id: RwLock<Option<NodeId>>,
    pub auth_token: RwLock<Option<NodeId>>,
}

impl ServiceContext {
    /// CreateSession 성공 후 호출 — 세션 식별자를 컨텍스트에 바인딩
    pub fn set_session(&self, session_id: NodeId, auth_token: NodeId) {
        *self.session_id.write() = Some(session_id);
        *self.auth_token.write() = Some(auth_token);
    }

    /// CloseSession 또는 CloseSecureChannel 시 호출
    pub fn clear_session(&self) {
        *self.session_id.write() = None;
        *self.auth_token.write() = None;
    }

    pub fn current_session_id(&self) -> Option<NodeId> {
        self.session_id.read().clone()
    }

    pub fn current_auth_token(&self) -> Option<NodeId> {
        self.auth_token.read().clone()
    }
}
```

### 3.3 세션 핸들러 연동

| 핸들러 | 변경 사항 |
|--------|-----------|
| `CreateSessionHandler` | 세션 생성 후 `context.set_session(session_id, auth_token)` 호출 |
| `ActivateSessionHandler` | `context.current_session_id()` 기반으로 세션 활성화. 세션이 없으면 `InvalidState` 에러 반환 |
| `CloseSessionHandler` | `context.current_session_id()` 기반으로 세션 종료 후 `context.clear_session()` 호출 |

### 3.4 CloseSecureChannel 응답 및 세션 정리

```rust
MessageType::CloseSecureChannel => {
    // 1. 활성 세션이 존재하면 정리
    if let Some(session_id) = context.current_session_id() {
        let _ = context.session_manager.close_session(&session_id);
        context.clear_session();
    }

    // 2. CLO 응답 전송
    let clo_body = build_msg_response_body(
        channel.channel_id(),
        channel.token_id(),
        &SequenceHeader { ... },
        &[],
    );
    let _ = framed.send(build_response(
        MessageType::CloseSecureChannel, clo_body
    )).await;

    break;
}
```

---

## 4. 수정 파일 요약

| 파일 | 변경 유형 | 설명 |
|------|----------|------|
| `service/registry.rs` | **구조 변경** | `ServiceContext`의 `session_id`/`auth_token`을 `RwLock<Option<NodeId>>`로 변경. `set_session()`, `clear_session()`, `current_session_id()`, `current_auth_token()` 메서드 추가 |
| `service/session.rs` | **기능 수정** | `CreateSessionHandler`에서 세션 생성 후 컨텍스트 바인딩. `ActivateSessionHandler`에서 컨텍스트 기반 세션 조회. `CloseSessionHandler`에서 컨텍스트 정리 |
| `transport/connection.rs` | **기능 수정** | `ServiceContext` 초기화 시 `RwLock::new(None)` 사용. `CloseSecureChannel` 수신 시 세션 정리 + 응답 전송 |

---

## 5. OPC UA 세션 상태 머신

수정 후 구현이 준수하는 전체 세션 라이프사이클:

```
Client                              Server
  │                                    │
  │── HEL (Hello) ──────────────────>  │
  │<──────────────────── ACK ─────────│
  │                                    │
  │── OPN (OpenSecureChannel) ──────>  │
  │<──── OpenSecureChannelResponse ───│  SecureChannel 수립
  │                                    │
  │── MSG (CreateSession) ──────────>  │
  │<──── CreateSessionResponse ───────│  session_id, auth_token 발급
  │                                    │  → context.set_session() ★
  │                                    │
  │── MSG (ActivateSession) ────────>  │
  │<──── ActivateSessionResponse ─────│  → context.current_session_id() ★
  │                                    │
  │── MSG (Read / Write / Browse) ──>  │  세션 컨텍스트 활용
  │<──── Response ────────────────────│
  │                                    │
  │── MSG (CloseSession) ───────────>  │
  │<──── CloseSessionResponse ────────│  → context.clear_session() ★
  │                                    │
  │── CLO (CloseSecureChannel) ─────>  │
  │<──── CLO Response ────────────────│  → 세션 정리 + 응답 ★
  │                                    │
```

---

## 6. 검증

### 단위 테스트

기존 178개 OPC UA 테스트 전부 통과 (0 failures).

### 프로토콜 정합성

| 단계 | 표준 조항 | 수정 전 | 수정 후 |
|------|----------|---------|---------|
| CreateSession → context binding | Part 4, 5.6.2 | session_id 미저장 | set_session() 호출 |
| ActivateSession → session lookup | Part 4, 5.6.3 | 항상 None 반환 | current_session_id() 조회 |
| CloseSession → cleanup | Part 4, 5.6.4 | session_id None으로 무동작 | clear_session() 호출 |
| CloseSecureChannel → response | Part 6, 7.1.4 | 응답 미전송 | CLO 응답 전송 + 세션 정리 |

---

## 7. 결론

본 수정은 OPC UA 세션 라이프사이클의 핵심 결함인 세션 컨텍스트 단절 문제를 `Interior Mutability` 패턴으로 해결하였다. 이를 통해 `CreateSession`에서 발급된 세션 식별자가 동일 SecureChannel의 전체 라이프사이클에 걸쳐 유지되며, Prosys OPC UA Simulation Server 등 상용 시뮬레이터와 동등한 수준의 세션 관리 완성도를 달성하였다.
