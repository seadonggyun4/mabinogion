//! Protocol-specific commands.
//!
//! Provides subcommands for each supported protocol.

use crate::context::CliContext;
use crate::error::{CliError, CliResult};
use crate::output::{OutputFormat, StatusType, TableBuilder};
use crate::runner::{Command, CommandOutput};
use async_trait::async_trait;
use mabi_core::prelude::*;
use serde::Serialize;
use std::net::SocketAddr;
use std::time::Duration;

/// Base trait for protocol-specific commands.
#[async_trait]
pub trait ProtocolCommand: Command {
    /// Get the protocol type.
    fn protocol(&self) -> Protocol;

    /// Get the default port.
    fn default_port(&self) -> u16;

    /// Start the protocol server.
    async fn start_server(&self, ctx: &mut CliContext) -> CliResult<()>;

    /// Stop the protocol server.
    async fn stop_server(&self, ctx: &mut CliContext) -> CliResult<()>;
}

// =============================================================================
// Modbus Command
// =============================================================================

/// Modbus protocol command.
pub struct ModbusCommand {
    /// Binding address.
    bind_addr: SocketAddr,
    /// Number of devices to simulate.
    devices: usize,
    /// Points per device.
    points_per_device: usize,
    /// Use RTU mode instead of TCP.
    rtu_mode: bool,
    /// Serial port for RTU mode.
    serial_port: Option<String>,
}

impl ModbusCommand {
    pub fn new() -> Self {
        Self {
            bind_addr: "0.0.0.0:502".parse().unwrap(),
            devices: 1,
            points_per_device: 100,
            rtu_mode: false,
            serial_port: None,
        }
    }

    pub fn with_bind_addr(mut self, addr: SocketAddr) -> Self {
        self.bind_addr = addr;
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.bind_addr.set_port(port);
        self
    }

    pub fn with_devices(mut self, devices: usize) -> Self {
        self.devices = devices;
        self
    }

    pub fn with_points(mut self, points: usize) -> Self {
        self.points_per_device = points;
        self
    }

    pub fn with_rtu_mode(mut self, serial_port: impl Into<String>) -> Self {
        self.rtu_mode = true;
        self.serial_port = Some(serial_port.into());
        self
    }
}

impl Default for ModbusCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for ModbusCommand {
    fn name(&self) -> &str {
        "modbus"
    }

    fn description(&self) -> &str {
        "Start a Modbus TCP/RTU simulator"
    }

    fn requires_engine(&self) -> bool {
        true
    }

    fn supports_shutdown(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &mut CliContext) -> CliResult<CommandOutput> {
        // Display configuration first
        {
            let output = ctx.output();
            if self.rtu_mode {
                output.header("Modbus RTU Simulator");
                output.kv(
                    "Serial Port",
                    self.serial_port.as_deref().unwrap_or("N/A"),
                );
            } else {
                output.header("Modbus TCP Simulator");
                output.kv("Bind Address", self.bind_addr);
            }
            output.kv("Devices", self.devices);
            output.kv("Points per Device", self.points_per_device);

            let total_points = self.devices * self.points_per_device;
            output.kv("Total Points", total_points);
        }

        // Start server
        self.start_server(ctx).await?;

        // Display status table
        let colors_enabled = ctx.colors_enabled();
        let table = TableBuilder::new(colors_enabled)
            .header(["Unit ID", "Holding Regs", "Input Regs", "Coils", "Discrete", "Status"])
            .status_row(
                [
                    "1",
                    &(self.points_per_device / 4).to_string(),
                    &(self.points_per_device / 4).to_string(),
                    &(self.points_per_device / 4).to_string(),
                    &(self.points_per_device / 4).to_string(),
                    "Online",
                ],
                StatusType::Success,
            );
        table.print();

        ctx.output().info("Press Ctrl+C to stop");

        // Wait for shutdown
        ctx.shutdown_signal().notified().await;

        self.stop_server(ctx).await?;
        ctx.output().success("Modbus simulator stopped");

        Ok(CommandOutput::quiet_success())
    }
}

#[async_trait]
impl ProtocolCommand for ModbusCommand {
    fn protocol(&self) -> Protocol {
        if self.rtu_mode {
            Protocol::ModbusRtu
        } else {
            Protocol::ModbusTcp
        }
    }

    fn default_port(&self) -> u16 {
        502
    }

    async fn start_server(&self, ctx: &mut CliContext) -> CliResult<()> {
        let output = ctx.output();
        let spinner = output.spinner("Starting Modbus server...");

        // TODO: Integrate with actual Modbus server from mabi-modbus
        tokio::time::sleep(Duration::from_millis(100)).await;

        spinner.finish_with_message(format!("Modbus server started on {}", self.bind_addr));
        Ok(())
    }

    async fn stop_server(&self, _ctx: &mut CliContext) -> CliResult<()> {
        // TODO: Stop actual server
        Ok(())
    }
}

// =============================================================================
// OPC UA Command
// =============================================================================

/// OPC UA protocol command.
pub struct OpcuaCommand {
    bind_addr: SocketAddr,
    endpoint_path: String,
    nodes: usize,
    security_mode: String,
}

impl OpcuaCommand {
    pub fn new() -> Self {
        Self {
            bind_addr: "0.0.0.0:4840".parse().unwrap(),
            endpoint_path: "/".into(),
            nodes: 1000,
            security_mode: "None".into(),
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.bind_addr.set_port(port);
        self
    }

    pub fn with_endpoint(mut self, path: impl Into<String>) -> Self {
        self.endpoint_path = path.into();
        self
    }

    pub fn with_nodes(mut self, nodes: usize) -> Self {
        self.nodes = nodes;
        self
    }

    pub fn with_security(mut self, mode: impl Into<String>) -> Self {
        self.security_mode = mode.into();
        self
    }
}

impl Default for OpcuaCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for OpcuaCommand {
    fn name(&self) -> &str {
        "opcua"
    }

    fn description(&self) -> &str {
        "Start an OPC UA server simulator"
    }

    fn requires_engine(&self) -> bool {
        true
    }

    fn supports_shutdown(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &mut CliContext) -> CliResult<CommandOutput> {
        // Display configuration first
        {
            let output = ctx.output();
            output.header("OPC UA Simulator");
            output.kv("Endpoint", format!("opc.tcp://{}{}", self.bind_addr, self.endpoint_path));
            output.kv("Nodes", self.nodes);
            output.kv("Security Mode", &self.security_mode);
        }

        self.start_server(ctx).await?;

        let colors_enabled = ctx.colors_enabled();
        let table = TableBuilder::new(colors_enabled)
            .header(["Namespace", "Nodes", "Subscriptions", "Status"])
            .status_row(["0", "Standard", "0", "Ready"], StatusType::Info)
            .status_row(
                ["1", &self.nodes.to_string(), "0", "Online"],
                StatusType::Success,
            );
        table.print();

        ctx.output().info("Press Ctrl+C to stop");
        ctx.shutdown_signal().notified().await;

        self.stop_server(ctx).await?;
        ctx.output().success("OPC UA simulator stopped");

        Ok(CommandOutput::quiet_success())
    }
}

#[async_trait]
impl ProtocolCommand for OpcuaCommand {
    fn protocol(&self) -> Protocol {
        Protocol::OpcUa
    }

    fn default_port(&self) -> u16 {
        4840
    }

    async fn start_server(&self, ctx: &mut CliContext) -> CliResult<()> {
        let output = ctx.output();
        let spinner = output.spinner("Starting OPC UA server...");

        // TODO: Integrate with actual OPC UA server
        tokio::time::sleep(Duration::from_millis(100)).await;

        spinner.finish_with_message("OPC UA server started");
        Ok(())
    }

    async fn stop_server(&self, _ctx: &mut CliContext) -> CliResult<()> {
        Ok(())
    }
}

// =============================================================================
// BACnet Command
// =============================================================================

/// BACnet protocol command.
pub struct BacnetCommand {
    bind_addr: SocketAddr,
    device_instance: u32,
    objects: usize,
    bbmd_enabled: bool,
}

impl BacnetCommand {
    pub fn new() -> Self {
        Self {
            bind_addr: "0.0.0.0:47808".parse().unwrap(),
            device_instance: 1234,
            objects: 100,
            bbmd_enabled: false,
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.bind_addr.set_port(port);
        self
    }

    pub fn with_device_instance(mut self, instance: u32) -> Self {
        self.device_instance = instance;
        self
    }

    pub fn with_objects(mut self, objects: usize) -> Self {
        self.objects = objects;
        self
    }

    pub fn with_bbmd(mut self, enabled: bool) -> Self {
        self.bbmd_enabled = enabled;
        self
    }
}

impl Default for BacnetCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for BacnetCommand {
    fn name(&self) -> &str {
        "bacnet"
    }

    fn description(&self) -> &str {
        "Start a BACnet/IP simulator"
    }

    fn requires_engine(&self) -> bool {
        true
    }

    fn supports_shutdown(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &mut CliContext) -> CliResult<CommandOutput> {
        // Display configuration first
        {
            let output = ctx.output();
            output.header("BACnet/IP Simulator");
            output.kv("Bind Address", self.bind_addr);
            output.kv("Device Instance", self.device_instance);
            output.kv("Objects", self.objects);
            output.kv("BBMD", if self.bbmd_enabled { "Enabled" } else { "Disabled" });
        }

        self.start_server(ctx).await?;

        let colors_enabled = ctx.colors_enabled();
        let table = TableBuilder::new(colors_enabled)
            .header(["Object Type", "Count", "Status"])
            .status_row(["Device", "1", "Online"], StatusType::Success)
            .status_row(["Analog Input", &(self.objects / 4).to_string(), "Active"], StatusType::Success)
            .status_row(["Analog Output", &(self.objects / 4).to_string(), "Active"], StatusType::Success)
            .status_row(["Binary Input", &(self.objects / 4).to_string(), "Active"], StatusType::Success)
            .status_row(["Binary Output", &(self.objects / 4).to_string(), "Active"], StatusType::Success);
        table.print();

        ctx.output().info("Press Ctrl+C to stop");
        ctx.shutdown_signal().notified().await;

        self.stop_server(ctx).await?;
        ctx.output().success("BACnet simulator stopped");

        Ok(CommandOutput::quiet_success())
    }
}

#[async_trait]
impl ProtocolCommand for BacnetCommand {
    fn protocol(&self) -> Protocol {
        Protocol::BacnetIp
    }

    fn default_port(&self) -> u16 {
        47808
    }

    async fn start_server(&self, ctx: &mut CliContext) -> CliResult<()> {
        let output = ctx.output();
        let spinner = output.spinner("Starting BACnet server...");

        // TODO: Integrate with actual BACnet server
        tokio::time::sleep(Duration::from_millis(100)).await;

        spinner.finish_with_message("BACnet server started");
        Ok(())
    }

    async fn stop_server(&self, _ctx: &mut CliContext) -> CliResult<()> {
        Ok(())
    }
}

// =============================================================================
// KNX Command
// =============================================================================

/// KNX protocol command.
pub struct KnxCommand {
    bind_addr: SocketAddr,
    individual_address: String,
    group_objects: usize,
}

impl KnxCommand {
    pub fn new() -> Self {
        Self {
            bind_addr: "0.0.0.0:3671".parse().unwrap(),
            individual_address: "1.1.1".into(),
            group_objects: 100,
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.bind_addr.set_port(port);
        self
    }

    pub fn with_individual_address(mut self, addr: impl Into<String>) -> Self {
        self.individual_address = addr.into();
        self
    }

    pub fn with_group_objects(mut self, count: usize) -> Self {
        self.group_objects = count;
        self
    }
}

impl Default for KnxCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for KnxCommand {
    fn name(&self) -> &str {
        "knx"
    }

    fn description(&self) -> &str {
        "Start a KNXnet/IP simulator"
    }

    fn requires_engine(&self) -> bool {
        true
    }

    fn supports_shutdown(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: &mut CliContext) -> CliResult<CommandOutput> {
        // Display configuration first
        {
            let output = ctx.output();
            output.header("KNXnet/IP Simulator");
            output.kv("Bind Address", self.bind_addr);
            output.kv("Individual Address", &self.individual_address);
            output.kv("Group Objects", self.group_objects);
        }

        self.start_server(ctx).await?;

        let colors_enabled = ctx.colors_enabled();
        let table = TableBuilder::new(colors_enabled)
            .header(["Service", "Status"])
            .status_row(["Core", "Ready"], StatusType::Success)
            .status_row(["Device Management", "Ready"], StatusType::Success)
            .status_row(["Tunneling", "Ready"], StatusType::Success);
        table.print();

        ctx.output().info("Press Ctrl+C to stop");
        ctx.shutdown_signal().notified().await;

        self.stop_server(ctx).await?;
        ctx.output().success("KNX simulator stopped");

        Ok(CommandOutput::quiet_success())
    }
}

#[async_trait]
impl ProtocolCommand for KnxCommand {
    fn protocol(&self) -> Protocol {
        Protocol::KnxIp
    }

    fn default_port(&self) -> u16 {
        3671
    }

    async fn start_server(&self, ctx: &mut CliContext) -> CliResult<()> {
        let output = ctx.output();
        let spinner = output.spinner("Starting KNX server...");

        // TODO: Integrate with actual KNX server
        tokio::time::sleep(Duration::from_millis(100)).await;

        spinner.finish_with_message("KNX server started");
        Ok(())
    }

    async fn stop_server(&self, _ctx: &mut CliContext) -> CliResult<()> {
        Ok(())
    }
}
