# mabi-modbus

Modbus TCP/RTU simulator for the Mabinogion industrial protocol simulator.

## Overview

High-performance Modbus protocol simulator supporting both TCP and RTU modes with scalable device simulation.

## Features

- Modbus TCP server simulation
- Modbus RTU (serial) simulation
- Multiple unit ID support
- Coils, discrete inputs, holding registers, input registers
- Scalable to 10,000+ devices
- Configurable response delays and error injection

## Usage

```rust
use mabi_modbus::prelude::*;

// Create a Modbus TCP server
let config = ModbusTcpConfig::builder()
    .bind_addr("0.0.0.0:502".parse()?)
    .unit_ids(vec![1, 2, 3])
    .build()?;

let server = ModbusTcpServer::new(config);
server.start().await?;
```

## License

Licensed under the Apache License, Version 2.0.
