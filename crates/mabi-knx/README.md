# mabi-knx

KNXnet/IP simulator for the Mabinogion industrial protocol simulator.

## Overview

KNXnet/IP protocol simulator for home and building automation testing.

## Features

- KNXnet/IP tunneling support
- Group address communication
- DPT (Datapoint Type) encoding/decoding
- Individual address assignment
- Connection management

## Usage

```rust
use mabi_knx::prelude::*;

// Create a KNX device
let config = KnxDeviceConfig::builder()
    .individual_address("1.1.1".parse()?)
    .port(3671)
    .build()?;

let device = KnxDevice::new(config);
device.start().await?;
```

## License

Licensed under the Apache License, Version 2.0.
