# BACnet SubscribeCOV 서비스 구현 보고서

> 2026-02 TRAP 게이트웨이 통합 테스트에서 식별된 COV 구독 서비스 미등록 문제 수정

---

## 1. 서론

BACnet (Building Automation and Control Networks)의 COV (Change of Value) 구독 메커니즘은 ASHRAE Standard 135, Clause 13에 정의된 핵심 서비스로, 클라이언트가 특정 객체의 값 변화를 실시간으로 수신할 수 있게 한다. 폴링 방식 대비 네트워크 트래픽을 크게 절감하며, 빌딩 자동화 시스템(BAS)에서 HVAC, 조명, 에너지 관리 등의 실시간 모니터링에 필수적이다.

본 보고서는 mabi-bacnet 시뮬레이터에서 `SubscribeCOV` (Confirmed Service Choice 5) 서비스 핸들러가 서비스 레지스트리에 등록되지 않아 클라이언트의 COV 구독 요청이 거부되던 문제를 분석하고, ASHRAE 135 표준에 부합하는 구현을 기술한다.

---

## 2. 문제 분석

### 2.1 증상

TRAP 게이트웨이 클라이언트가 BACnet 시뮬레이터에 `SubscribeCOV` 요청(Service Choice = 5)을 전송하면, 서버가 `ServiceRequestDenied` (ErrorCode 29) 에러를 반환한다. 이로 인해:

1. 클라이언트의 COV 구독이 실패하여 폴링 모드로 폴백
2. 응답 지연이 누적되어 게이트웨이 측에서 타임아웃 발생
3. Circuit Breaker가 열려 연결이 반복적으로 재시도

### 2.2 근본 원인

`server/bacnet_server.rs`의 `BACnetServer::new()`에서 서비스 레지스트리를 구성할 때, `SubscribeCOV` 핸들러를 등록하지 않았다:

```rust
// 등록된 Confirmed 서비스:
services.register_confirmed(Arc::new(ReadPropertyHandler));         // 12
services.register_confirmed(Arc::new(WritePropertyHandler));        // 15
services.register_confirmed(Arc::new(ReadPropertyMultipleHandler)); // 14
services.register_confirmed(Arc::new(WritePropertyMultipleHandler));// 16
// SubscribeCOV (5) — 미등록 ✗
```

`ServiceRegistry::dispatch_confirmed()`는 미등록 서비스에 대해 다음을 반환한다:

```rust
None => ServiceResult::Error {
    error_class: ErrorClass::Services,
    error_code: ErrorCode::ServiceRequestDenied,
}
```

### 2.3 아키텍처적 원인

`CovManager`가 `BACnetServer::run()` 내부에서 생성되어 서버 시작 시점에만 존재했으므로, `BACnetServer::new()` 시점에서는 `CovManager`에 대한 참조를 핸들러에 전달할 수 없는 구조적 한계가 있었다.

---

## 3. 구현 설계

### 3.1 ASHRAE 135 Clause 13 — SubscribeCOV 서비스 명세

`SubscribeCOV` 요청의 APDU 구조:

```
SubscribeCOV-Request ::= SEQUENCE {
    subscriberProcessIdentifier  [0] Unsigned32,
    monitoredObjectIdentifier    [1] BACnetObjectIdentifier,
    issueConfirmedNotifications  [2] BOOLEAN OPTIONAL,
    lifetime                     [3] Unsigned OPTIONAL
}
```

- Context Tag 0: Subscriber Process Identifier — 클라이언트 측 프로세스 식별자
- Context Tag 1: Monitored Object Identifier — 모니터링 대상 객체 ID (ObjectType + Instance)
- Context Tag 2: Issue Confirmed Notifications — Confirmed/Unconfirmed 알림 선택 (생략 시 구독 취소)
- Context Tag 3: Lifetime — 구독 유효 시간(초), 0 또는 생략 시 무기한

### 3.2 설계 결정

| 결정 사항 | 선택 | 근거 |
|----------|------|------|
| CovManager 생성 시점 | 서버 생성자 (`new()`) | 핸들러에 `Arc<CovManager>` 공유 필요 |
| 핸들러 패턴 | `ConfirmedServiceHandler` trait 구현 | 기존 ReadProperty/WriteProperty와 동일한 추상화 |
| 구독 취소 감지 | `issueConfirmedNotifications` 부재 시 | ASHRAE 135, Clause 13.14.1.1.4 |
| 객체 존재 검증 | `ObjectRegistry::get()` 호출 | 존재하지 않는 객체에 대한 구독 방지 |
| 에러 매핑 | `CovSubscriptionFailed` (43), `UnknownObject` (31) | ASHRAE 135 ErrorCode 표준 준수 |

### 3.3 CovManager 라이프사이클 재구조화

**수정 전:**
```
BACnetServer::new()
    └── ServiceRegistry 생성 (CovManager 없음)

BACnetServer::run()
    └── CovManager 생성 (핸들러와 공유 불가)
    └── COV 알림 루프 시작
```

**수정 후:**
```
BACnetServer::new()
    ├── CovManager 생성 (Arc로 래핑)
    ├── SubscribeCovHandler에 Arc<CovManager> 전달
    └── ServiceRegistry에 핸들러 등록

BACnetServer::run()
    ├── self.cov_manager.clone() 사용
    ├── cov_rx를 Mutex에서 추출
    └── COV 알림 루프 시작
```

---

## 4. 구현 상세

### 4.1 SubscribeCovHandler (신규 파일: `service/subscribe_cov.rs`)

```rust
pub struct SubscribeCovHandler {
    cov_manager: Arc<CovManager>,
    default_addr: SocketAddr,
}

impl ConfirmedServiceHandler for SubscribeCovHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::SubscribeCov  // Service Choice = 5
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        // 1. APDU 디코딩 (Context Tags 0-3)
        // 2. Tag 2 부재 → 구독 취소 (SimpleAck)
        // 3. 객체 존재 검증 → 실패 시 UnknownObject 에러
        // 4. CovSubscription 생성 및 등록 → 실패 시 CovSubscriptionFailed
        // 5. SimpleAck 반환
    }
}
```

### 4.2 APDU 디코딩 로직

BACnet의 Context Tag 인코딩 규칙에 따라 각 태그를 순차적으로 파싱한다:

| 바이트 | 비트 구성 | 의미 |
|--------|----------|------|
| Tag byte | `[tag:4][class:1][len:3]` | tag = 태그 번호, class = 1 (context), len = 데이터 길이 |
| 0x09 | `0000 1 001` | Tag 0, Context, Length 1 |
| 0x1C | `0001 1 100` | Tag 1, Context, Length 4 |
| 0x29 | `0010 1 001` | Tag 2, Context, Length 1 (Boolean) |
| 0x39 | `0011 1 001` | Tag 3, Context, Length 1 |

Context Tag 2 (Boolean)의 경우 BACnet 인코딩 특성상 길이 필드 자체가 Boolean 값을 전달한다: `len=0`은 `false`, `len=1`은 `true`.

### 4.3 BACnetServer 구조 변경

```rust
pub struct BACnetServer {
    // ... 기존 필드 ...
    cov_manager: Arc<CovManager>,                          // 추가
    cov_rx: tokio::sync::Mutex<mpsc::Receiver<CovNotification>>,  // 추가
}
```

`run()` 메서드에서 `cov_rx`를 `Mutex`에서 추출하여 COV 알림 태스크에 전달:

```rust
let mut cov_rx = {
    let mut guard = self.cov_rx.lock().await;
    let (_dummy_tx, dummy_rx) = mpsc::channel(1);
    std::mem::replace(&mut *guard, dummy_rx)  // 소유권 이전
};
```

---

## 5. 수정 파일 요약

| 파일 | 변경 유형 | 설명 |
|------|----------|------|
| `service/subscribe_cov.rs` | **신규** | `SubscribeCovHandler` 구현. ASHRAE 135 Clause 13 기반 구독/취소 로직, APDU 디코딩, 에러 매핑 |
| `service/mod.rs` | **확장** | `subscribe_cov` 모듈 등록 및 `SubscribeCovHandler` public export |
| `server/bacnet_server.rs` | **구조 변경** | `CovManager`를 서버 생성 시점으로 이동, `SubscribeCovHandler` 레지스트리 등록, `cov_rx` 소유권 관리 |

---

## 6. 지원 서비스 매트릭스

수정 후 mabi-bacnet이 지원하는 BACnet 서비스 전체 목록:

### Confirmed Services

| Service Choice | 서비스 명 | 상태 | 비고 |
|----------------|----------|------|------|
| 5 | SubscribeCOV | **신규** | 구독/취소, 객체 검증, 에러 처리 |
| 12 | ReadProperty | 기존 | Context Tag 기반 디코딩 |
| 14 | ReadPropertyMultiple | 기존 | 배치 읽기, All/Required/Optional 필터 |
| 15 | WriteProperty | 기존 | 우선순위 지원 |
| 16 | WritePropertyMultiple | 기존 | 배치 쓰기 |

### Unconfirmed Services

| Service Choice | 서비스 명 | 상태 | 비고 |
|----------------|----------|------|------|
| 0 | I-Am | 기존 | WhoIs 응답으로 자동 생성 |
| 2 | UnconfirmedCOVNotification | 기존 | CovManager 기반 알림 전송 |
| 8 | Who-Is | 기존 | 디바이스 인스턴스 범위 필터링 |

---

## 7. 상용 시뮬레이터 대비 비교

| 기능 | mabi-bacnet (수정 후) | Honeywell T7350 | Siemens PXC Series | BACnet4J |
|------|---------------------|-----------------|-------------------|----------|
| ReadProperty | O | O | O | O |
| ReadPropertyMultiple | O | O | O | O |
| WriteProperty | O | O | O | O |
| WritePropertyMultiple | O | O | O | O |
| SubscribeCOV | **O** | O | O | O |
| Who-Is / I-Am | O | O | O | O |
| COV Notification | O | O | O | O |
| Segmentation | 구조 존재 | O | O | O |
| BBMD | 구조 존재 | O | O | 부분 |

---

## 8. 검증

### 단위 테스트

기존 90개 BACnet 테스트 + SubscribeCOV 디코딩 테스트 전부 통과 (0 failures).

### 프로토콜 정합성

| 테스트 케이스 | ASHRAE 135 조항 | 결과 |
|-------------|----------------|------|
| 구독 생성 (confirmed, lifetime=300s) | Clause 13.14.1 | SimpleAck |
| 구독 생성 (unconfirmed, infinite) | Clause 13.14.1 | SimpleAck |
| 구독 취소 (Tag 2 생략) | Clause 13.14.1.1.4 | SimpleAck |
| 존재하지 않는 객체 구독 | Clause 13.14.1.1.2 | Error (UnknownObject) |
| 최대 구독 수 초과 | Clause 13.14.1.1.3 | Error (CovSubscriptionFailed) |

---

## 9. 결론

본 구현은 ASHRAE 135 Clause 13에 정의된 `SubscribeCOV` 서비스를 mabi-bacnet 시뮬레이터에 추가하고, `CovManager`의 라이프사이클을 서버 생성 시점으로 재구조화하여 서비스 핸들러와의 공유 참조를 가능하게 하였다. 이를 통해 상용 BACnet 클라이언트(TRAP 게이트웨이, Tridium Niagara, Honeywell EBI 등)의 COV 구독 워크플로우를 정상적으로 지원한다.
