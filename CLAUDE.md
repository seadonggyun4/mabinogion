# CLAUDE.md - Mabinogion

> **Industrial Protocol Simulator**
>
> Modbus/OPC UA/BACnet/KNX 가상 디바이스 시뮬레이터

## 🎯 Project Overview

Mabinogion은 산업용 프로토콜 클라이언트의 **스트레스 테스트**, **엣지 케이스 검증**, **대용량 데이터 처리 능력**을 검증하기 위한 프로토콜 시뮬레이터입니다.

### 핵심 목표

1. **대용량 시뮬레이션**: 10,000+ 가상 디바이스, 1,000,000+ 데이터 포인트 동시 처리
2. **프로토콜 정합성**: 실제 산업 환경의 프로토콜 동작을 정확히 재현
3. **카오스 엔지니어링**: 네트워크 장애, 디바이스 오류, 지연 등 장애 상황 시뮬레이션
4. **재현 가능한 테스트**: 동일 시나리오의 반복 실행으로 일관된 테스트 결과

## 📁 Project Structure

```
mabinogion/
├── CLAUDE.md                    # 이 문서
├── Cargo.toml                   # 워크스페이스 루트
├── .claude/
│   └── ROADMAP.md              # 전체 개발 로드맵
├── docs/                        # 상세 문서 (아래 테이블 참조)
├── crates/
│   ├── mabi-core/               # 공통 추상화, 메트릭, 유틸리티
│   ├── mabi-modbus/             # Modbus TCP/RTU 시뮬레이터
│   ├── mabi-opcua/              # OPC UA 서버 시뮬레이터
│   ├── mabi-bacnet/             # BACnet/IP 시뮬레이터
│   ├── mabi-knx/                # KNXnet/IP 시뮬레이터
│   ├── mabi-scenario/           # 시나리오 엔진
│   ├── mabi-chaos/              # 카오스 엔지니어링 모듈
│   └── mabi-cli/                # CLI 바이너리 (mabi)
├── scenarios/                   # 시나리오 파일들
├── tests/
│   ├── integration/            # 통합 테스트 프레임워크
│   └── stress/                 # 스트레스 테스트 프레임워크
└── benches/                     # 벤치마크
```

## 📚 Module Documentation

모든 모듈의 상세 문서는 `docs/` 디렉토리에 있습니다. 아키텍처, API, 설정, 사용 예제 등을 포함합니다.

### 프로토콜 시뮬레이터

| 모듈 | 설명 | 주요 내용 | 문서 |
|------|------|----------|------|
| **Modbus TCP/RTU** | 공장 자동화, PLC, 센서 | TCP/RTU 이중 모드, 핸들러 레지스트리, 스파스 레지스터, 멀티유닛, 콜백, 결함 주입 | [docs/modbus-simulator](./docs/modbus-simulator/README.md) |
| **OPC UA** | 산업 IoT, SCADA | 주소 공간, 구독, 히스토리 접근(22+ 집계함수), 보안 정책, 노드 캐시 | [docs/opcua-simulator](./docs/opcua-simulator/README.md) |
| **BACnet/IP** | 빌딩 자동화 (HVAC, 조명) | 9개 객체 타입, COV 구독, 우선순위 배열, BBMD, APDU 세그먼테이션, 서비스 핸들러 | [docs/bacnet-simulator](./docs/bacnet-simulator/README.md) |
| **KNXnet/IP** | 스마트홈/빌딩 시스템 | 터널링, 25+ 데이터포인트 타입, 그룹 주소, DPT 코덱, cEMI, 연결 관리 | [docs/knx-simulator](./docs/knx-simulator/README.md) |

### 코어 & 인프라

| 모듈 | 설명 | 주요 내용 | 문서 |
|------|------|----------|------|
| **Core** | 공통 추상화, 유틸리티 | Device trait, SimulatorEngine, 팩토리 시스템, 메트릭(Prometheus), 라이프사이클, 캐퍼빌리티, 에러 타입 | [docs/core](./docs/core/README.md) |
| **Scenario Engine** | 시나리오 기반 시뮬레이션 | 9개 패턴 타입(Sine, Ramp, Step 등), 이벤트 트리거/액션, 시간 스케일링, YAML 스키마, 리플레이 | [docs/scenario-engine](./docs/scenario-engine/README.md) |
| **Chaos Engine** | 카오스 엔지니어링 | 네트워크/디바이스/프로토콜 결함(7 카테고리), 지연 분포 모델(5+), 스케줄러, 미들웨어, YAML 설정 | [docs/chaos-engine](./docs/chaos-engine/README.md) |
| **CLI** | 명령줄 인터페이스 | 전체 명령어 레퍼런스, 글로벌 옵션, 출력 포맷(table/json/yaml), exit code(10종), 입력 검증 | [docs/cli](./docs/cli/README.md) |

## 🛠 Tech Stack

| Category | Technology | Purpose |
|----------|------------|---------|
| Language | Rust 1.75+ | 고성능, 메모리 안전성 |
| Async Runtime | Tokio | 비동기 I/O, 대규모 동시성 |
| Modbus | tokio-modbus | Modbus TCP/RTU 구현 |
| OPC UA | 커스텀 구현 | OPC UA 서버 구현 |
| BACnet | 커스텀 구현 | BACnet/IP 구현 |
| KNX | 커스텀 구현 | KNXnet/IP 구현 |
| CLI | clap | 명령줄 인터페이스 |
| Config | serde + YAML | 시나리오 파싱 |
| Logging | tracing | 구조화된 로깅 |

## 🖥 CLI Usage

```bash
# 시나리오 실행
mabi run scenario.yaml
mabi run scenario.yaml --time-scale 2.0 --duration 10m
mabi run scenario.yaml --dry-run

# 프로토콜 서버 단독 실행
mabi modbus --port 5020 --devices 10 --points 1000
mabi modbus --rtu --serial /dev/ttyUSB0
mabi opcua --port 4840 --nodes 10000
mabi bacnet --port 47808 --instance 1234 --objects 100
mabi knx --port 3671 --address 1.1.1

# 검증 / 목록 / 디버깅
mabi validate scenario.yaml
mabi validate --strict *.yaml
mabi list protocols --format json
mabi list devices --protocol modbus
mabi -v run scenario.yaml       # verbose
mabi -vv run scenario.yaml      # debug
```

> 전체 CLI 레퍼런스: [docs/cli](./docs/cli/README.md)

## 🔧 Development Guidelines

### Commit Convention

```
<type>(<scope>): <description>
```

**Types:** `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `chore`

**Scopes:** `core`, `modbus`, `opcua`, `bacnet`, `knx`, `scenario`, `chaos`, `cli`

> **Note:** 모든 인터페이스는 CLI로 통일. Web API는 지원하지 않음.

### Code Style

```rust
// 1. 모든 public API에 문서화 필수
/// Creates a new Modbus TCP simulator.
pub fn new(config: ModbusConfig) -> Result<Self> { ... }

// 2. Error 타입은 thiserror 사용
#[derive(Debug, thiserror::Error)]
pub enum SimulatorError {
    #[error("Device not found: {device_id}")]
    DeviceNotFound { device_id: String },
}

// 3. Builder 패턴 적극 활용
let device = DeviceBuilder::new("ahu-01")
    .protocol(Protocol::BACnet)
    .build()?;
```

### Performance Requirements

| Metric | Target |
|--------|--------|
| 디바이스 수 | 10,000+ |
| 데이터 포인트 | 1,000,000+ |
| 메시지 처리량 | 100,000 msg/s |
| 메모리 사용량 | < 2GB (10K devices) |
| 지연시간 (p99) | < 10ms |

### Testing

```bash
cargo test --workspace              # 단위 테스트
cargo test --test integration       # 통합 테스트
cargo test --package mabi-cli       # CLI 테스트
cargo test --test e2e_tests         # E2E 테스트
cargo bench                         # 벤치마크
```

## 🚨 Critical Implementation Notes

### 1. 메모리 관리

```rust
// ✅ 스트리밍 방식으로 처리
let data_stream = scenario.stream_data();
while let Some(batch) = data_stream.next().await {
    process_batch(batch).await?;
}
```

### 2. 동시성 안전성

```rust
// ✅ 샤딩된 동시성 맵 사용
use dashmap::DashMap;
let devices: DashMap<String, Device> = DashMap::new();
```

### 3. 에러 복구

```rust
// ✅ 개별 디바이스 에러 격리 및 복구
match device.process().await {
    Ok(_) => metrics.record_success(),
    Err(e) => {
        metrics.record_error(&e);
        device.attempt_recovery().await;
    }
}
```

## 📋 Phase Overview

```
Phase 1: Core Architecture ✅
    ↓
Phase 2-5: Protocol Simulators (Modbus ✅, OPC UA ✅, BACnet ✅, KNX ✅)
    ↓
Phase 6: Scenario Engine ✅
    ↓
Phase 7: Chaos Engineering ✅
    ↓
Phase 8: CLI & Integration Testing ✅
```

**모든 Phase 완료 (100%)**

---

**Last Updated**: 2026-01-30
**Version**: 0.1.0
