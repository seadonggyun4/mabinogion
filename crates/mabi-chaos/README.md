# mabi-chaos

Chaos engineering module for the Mabinogion industrial protocol simulator.

## Overview

Fault injection and resilience testing framework for industrial protocol clients.

## Features

- Network fault injection (latency, packet loss, corruption)
- Device failure simulation
- Protocol error injection
- Scheduled chaos events
- Configurable fault patterns

## Fault Types

| Category | Faults |
|----------|--------|
| Network | Latency, packet loss, bandwidth throttling, connection drops |
| Device | Offline, timeout, corrupted responses |
| Protocol | Invalid CRC, malformed packets, out-of-sequence |

## Usage

```rust
use mabi_chaos::prelude::*;

// Create a chaos engine
let engine = ChaosEngine::builder()
    .add_fault(LatencyFault::new(Duration::from_millis(100)))
    .add_fault(PacketLossFault::new(0.05)) // 5% loss
    .build()?;

engine.start().await?;
```

## License

Licensed under the Apache License, Version 2.0.
