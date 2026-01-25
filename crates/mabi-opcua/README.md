# mabi-opcua

OPC UA server simulator for the Mabinogion industrial protocol simulator.

## Overview

Full-featured OPC UA server simulator with support for subscriptions, historical data, and security.

## Features

- OPC UA server implementation
- Address space management
- Subscription and monitored items
- Historical data access
- Security modes (None, Sign, SignAndEncrypt)
- Scalable node management

## Usage

```rust
use mabi_opcua::prelude::*;

// Create an OPC UA server
let config = OpcUaServerConfig::builder()
    .port(4840)
    .endpoint_path("/")
    .build()?;

let server = OpcUaServer::new(config);
server.start().await?;
```

## License

Licensed under the Apache License, Version 2.0.
