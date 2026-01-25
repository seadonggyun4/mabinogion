# mabi-bacnet

BACnet/IP simulator for the Mabinogion industrial protocol simulator.

## Overview

BACnet/IP protocol simulator for building automation system testing.

## Features

- BACnet/IP device simulation
- Object types: Analog/Binary/Multi-state Input/Output/Value
- Property read/write services
- COV (Change of Value) subscriptions
- Who-Is/I-Am discovery
- BBMD (BACnet Broadcast Management Device) support

## Usage

```rust
use mabi_bacnet::prelude::*;

// Create a BACnet device
let config = BacnetDeviceConfig::builder()
    .device_instance(1234)
    .port(47808)
    .build()?;

let device = BacnetDevice::new(config);
device.start().await?;
```

## License

Licensed under the Apache License, Version 2.0.
