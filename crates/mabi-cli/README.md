# mabi-cli

Command-line interface for the Mabinogion industrial protocol simulator.

## Installation

```bash
cargo install mabi-cli
```

## Usage

```bash
# Start Modbus TCP server
mabi modbus --port 502 --devices 10 --points 100

# Start OPC UA server
mabi opcua --port 4840 --nodes 1000

# Start BACnet/IP server
mabi bacnet --port 47808 --instance 1234

# Start KNXnet/IP server
mabi knx --port 3671 --address 1.1.1

# Run scenario
mabi run scenario.yaml

# Run with time scaling
mabi run scenario.yaml --time-scale 2.0 --duration 10m

# Validate scenario
mabi validate scenario.yaml

# List resources
mabi list protocols
mabi list devices --format json
```

## Commands

| Command | Description |
|---------|-------------|
| `mabi modbus` | Start Modbus TCP/RTU server |
| `mabi opcua` | Start OPC UA server |
| `mabi bacnet` | Start BACnet/IP server |
| `mabi knx` | Start KNXnet/IP server |
| `mabi run` | Run simulation scenario |
| `mabi validate` | Validate scenario file |
| `mabi list` | List resources |

## License

Licensed under the Apache License, Version 2.0.
