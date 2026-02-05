//! Protocol-specific commands.
//!
//! Provides subcommands for each supported protocol.

use crate::context::CliContext;
use crate::error::CliResult;
use crate::output::{OutputFormat, PaginatedTable, StatusType, TableBuilder};
use crate::runner::{Command, CommandOutput};
use async_trait::async_trait;
use mabi_core::prelude::*;
use mabi_core::tags::Tags;
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

// Protocol-specific imports
use mabi_modbus::{ModbusTcpServerV2, tcp::ServerConfigV2, ModbusDevice, ModbusDeviceConfig};
use mabi_opcua::{OpcUaServer, OpcUaServerConfig};
use mabi_bacnet::prelude::{BACnetServer, ServerConfig as BacnetServerConfig, ObjectRegistry, AnalogInput, AnalogOutput, BinaryInput, BinaryOutput};
use mabi_knx::{KnxServer, KnxServerConfig, IndividualAddress, GroupAddress, DptId, GroupObjectTable};

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
    /// Device tags.
    tags: Tags,
    /// Server instance (for shutdown).
    server: Arc<Mutex<Option<Arc<ModbusTcpServerV2>>>>,
    /// Server task handle.
    server_task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl ModbusCommand {
    pub fn new() -> Self {
        Self {
            bind_addr: "0.0.0.0:502".parse().unwrap(),
            devices: 1,
            points_per_device: 100,
            rtu_mode: false,
            serial_port: None,
            tags: Tags::new(),
            server: Arc::new(Mutex::new(None)),
            server_task: Arc::new(Mutex::new(None)),
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

    pub fn with_tags(mut self, tags: Tags) -> Self {
        self.tags = tags;
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
        let format = ctx.output().format();
        let is_quiet = ctx.is_quiet();
        let is_verbose = ctx.is_verbose();
        let is_debug = ctx.is_debug();

        if !is_quiet {
            if matches!(format, OutputFormat::Table) {
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
                output.kv("Total Points", self.devices * self.points_per_device);
            }
        }

        // Verbose: show extra configuration details
        if is_verbose {
            ctx.vprintln(format!("  Protocol Mode: {}", if self.rtu_mode { "RTU" } else { "TCP" }));
            ctx.vprintln(format!("  Points Distribution: {} per register type", self.points_per_device / 4));
        }

        // Debug: dump full configuration
        if is_debug {
            ctx.dprintln(format!("Bind address: {}", self.bind_addr));
            ctx.dprintln(format!("RTU mode: {}, Serial: {:?}", self.rtu_mode, self.serial_port));
            ctx.dprintln(format!("Devices: {}, Points/device: {}", self.devices, self.points_per_device));
        }

        self.start_server(ctx).await?;

        let points_per_type = self.points_per_device / 4;

        if !is_quiet {
            match format {
                OutputFormat::Table => {
                    let colors_enabled = ctx.colors_enabled();
                    let builder = TableBuilder::new(colors_enabled)
                        .header(["Unit ID", "Holding Regs", "Input Regs", "Coils", "Discrete", "Status"]);

                    let devices = self.devices;
                    let pts = points_per_type.to_string();
                    let table = PaginatedTable::default().render(builder, devices, 6, |i| {
                        let unit_id = (i + 1).to_string();
                        (
                            vec![unit_id, pts.clone(), pts.clone(), pts.clone(), pts.clone(), "Online".into()],
                            StatusType::Success,
                        )
                    });
                    table.print();
                }
                _ => {
                    #[derive(Serialize)]
                    struct ModbusServerInfo {
                        protocol: String,
                        bind_address: String,
                        devices: usize,
                        points_per_device: usize,
                        total_points: usize,
                        rtu_mode: bool,
                        serial_port: Option<String>,
                        device_list: Vec<ModbusDeviceInfo>,
                        status: String,
                    }
                    #[derive(Serialize)]
                    struct ModbusDeviceInfo {
                        unit_id: usize,
                        holding_registers: usize,
                        input_registers: usize,
                        coils: usize,
                        discrete_inputs: usize,
                        status: String,
                    }
                    let device_list: Vec<ModbusDeviceInfo> = (0..self.devices)
                        .map(|i| ModbusDeviceInfo {
                            unit_id: i + 1,
                            holding_registers: points_per_type,
                            input_registers: points_per_type,
                            coils: points_per_type,
                            discrete_inputs: points_per_type,
                            status: "Online".into(),
                        })
                        .collect();
                    let info = ModbusServerInfo {
                        protocol: if self.rtu_mode { "Modbus RTU".into() } else { "Modbus TCP".into() },
                        bind_address: self.bind_addr.to_string(),
                        devices: self.devices,
                        points_per_device: self.points_per_device,
                        total_points: self.devices * self.points_per_device,
                        rtu_mode: self.rtu_mode,
                        serial_port: self.serial_port.clone(),
                        device_list,
                        status: "Online".into(),
                    };
                    let _ = ctx.output().write(&info);
                }
            }
        }

        if !is_quiet {
            ctx.output().info("Press Ctrl+C to stop");
        }
        ctx.shutdown_signal().notified().await;

        self.stop_server(ctx).await?;
        if !is_quiet {
            ctx.output().success("Modbus simulator stopped");
        }

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

        let config = ServerConfigV2 {
            bind_address: self.bind_addr,
            ..Default::default()
        };

        let server = Arc::new(ModbusTcpServerV2::new(config));

        for i in 0..self.devices {
            let unit_id = (i + 1) as u8;
            let points = (self.points_per_device / 4) as u16;
            let device_config = ModbusDeviceConfig {
                unit_id,
                name: format!("Device-{}", unit_id),
                holding_registers: points,
                input_registers: points,
                coils: points,
                discrete_inputs: points,
                response_delay_ms: 0,
                tags: self.tags.clone(),
            };
            let device = ModbusDevice::new(device_config);
            server.add_device(device);
        }

        {
            let mut server_guard = self.server.lock().await;
            *server_guard = Some(server.clone());
        }

        let server_clone = server.clone();
        let task = tokio::spawn(async move {
            if let Err(e) = server_clone.run().await {
                tracing::error!("Modbus server error: {}", e);
            }
        });

        {
            let mut task_guard = self.server_task.lock().await;
            *task_guard = Some(task);
        }

        tokio::time::sleep(Duration::from_millis(100)).await;

        spinner.finish_with_message(format!("Modbus server started on {}", self.bind_addr));
        Ok(())
    }

    async fn stop_server(&self, _ctx: &mut CliContext) -> CliResult<()> {
        if let Some(server) = self.server.lock().await.as_ref() {
            server.shutdown();
        }

        if let Some(task) = self.server_task.lock().await.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), task).await;
        }

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
    /// Device tags.
    tags: Tags,
    /// Server instance (for shutdown).
    server: Arc<Mutex<Option<Arc<OpcUaServer>>>>,
    /// Server task handle.
    server_task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl OpcuaCommand {
    pub fn new() -> Self {
        Self {
            bind_addr: "0.0.0.0:4840".parse().unwrap(),
            endpoint_path: "/".into(),
            nodes: 1000,
            security_mode: "None".into(),
            tags: Tags::new(),
            server: Arc::new(Mutex::new(None)),
            server_task: Arc::new(Mutex::new(None)),
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

    pub fn with_tags(mut self, tags: Tags) -> Self {
        self.tags = tags;
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
        let format = ctx.output().format();
        let is_quiet = ctx.is_quiet();
        let is_verbose = ctx.is_verbose();
        let is_debug = ctx.is_debug();

        if !is_quiet {
            if matches!(format, OutputFormat::Table) {
                let output = ctx.output();
                output.header("OPC UA Simulator");
                output.kv("Endpoint", format!("opc.tcp://{}{}", self.bind_addr, self.endpoint_path));
                output.kv("Nodes", self.nodes);
                output.kv("Security Mode", &self.security_mode);
            }
        }

        // Verbose: show extra details
        if is_verbose {
            ctx.vprintln(format!("  Bind Address: {}", self.bind_addr));
            ctx.vprintln(format!("  Endpoint Path: {}", self.endpoint_path));
            ctx.vprintln(format!("  Max Subscriptions: 1000"));
            ctx.vprintln(format!("  Max Monitored Items: 10000"));
        }

        // Debug: dump full configuration
        if is_debug {
            ctx.dprintln(format!("Full endpoint URL: opc.tcp://{}{}", self.bind_addr, self.endpoint_path));
            ctx.dprintln(format!("Node count: {}", self.nodes));
            ctx.dprintln(format!("Security mode: {}", self.security_mode));
            ctx.dprintln(format!("Sample nodes created: {}", self.nodes.min(100)));
        }

        self.start_server(ctx).await?;

        if !is_quiet {
            match format {
                OutputFormat::Table => {
                    let colors_enabled = ctx.colors_enabled();
                    let table = TableBuilder::new(colors_enabled)
                        .header(["Namespace", "Nodes", "Subscriptions", "Status"])
                        .status_row(["0", "Standard", "0", "Ready"], StatusType::Info)
                        .status_row(
                            ["1", &self.nodes.to_string(), "0", "Online"],
                            StatusType::Success,
                        );
                    table.print();
                }
                _ => {
                    #[derive(Serialize)]
                    struct OpcuaServerInfo {
                        protocol: String,
                        endpoint: String,
                        nodes: usize,
                        security_mode: String,
                        namespaces: Vec<NamespaceInfo>,
                        status: String,
                    }
                    #[derive(Serialize)]
                    struct NamespaceInfo {
                        index: u32,
                        nodes: String,
                        subscriptions: u32,
                        status: String,
                    }
                    let info = OpcuaServerInfo {
                        protocol: "OPC UA".into(),
                        endpoint: format!("opc.tcp://{}{}", self.bind_addr, self.endpoint_path),
                        nodes: self.nodes,
                        security_mode: self.security_mode.clone(),
                        namespaces: vec![
                            NamespaceInfo { index: 0, nodes: "Standard".into(), subscriptions: 0, status: "Ready".into() },
                            NamespaceInfo { index: 1, nodes: self.nodes.to_string(), subscriptions: 0, status: "Online".into() },
                        ],
                        status: "Online".into(),
                    };
                    let _ = ctx.output().write(&info);
                }
            }
        }

        if !is_quiet {
            ctx.output().info("Press Ctrl+C to stop");
        }
        ctx.shutdown_signal().notified().await;

        self.stop_server(ctx).await?;
        if !is_quiet {
            ctx.output().success("OPC UA simulator stopped");
        }

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

        let config = OpcUaServerConfig {
            endpoint_url: format!("opc.tcp://{}{}", self.bind_addr, self.endpoint_path),
            server_name: "Mabinogion OPC UA Simulator".to_string(),
            max_subscriptions: 1000,
            max_monitored_items: 10000,
            ..Default::default()
        };

        let server = Arc::new(OpcUaServer::new(config).map_err(|e| {
            crate::error::CliError::ExecutionFailed {
                message: format!("Failed to create OPC UA server: {}", e)
            }
        })?);

        // Add sample nodes with diverse types and mixed read-only / writable access.
        // Even-indexed nodes are writable, odd-indexed are read-only.
        let node_count = self.nodes.min(100);
        for i in 0..node_count {
            let node_id = format!("ns=2;i={}", 1000 + i);
            let name = format!("Variable_{}", i);
            let value = (i as f64) * 0.1;

            if i % 2 == 0 {
                let _ = server.add_writable_variable(node_id, name, value);
            } else {
                let _ = server.add_variable(node_id, name, value);
            }
        }

        {
            let mut server_guard = self.server.lock().await;
            *server_guard = Some(server.clone());
        }

        let server_clone = server.clone();
        let task = tokio::spawn(async move {
            if let Err(e) = server_clone.start().await {
                tracing::error!("OPC UA server error: {}", e);
            }
        });

        {
            let mut task_guard = self.server_task.lock().await;
            *task_guard = Some(task);
        }

        tokio::time::sleep(Duration::from_millis(100)).await;

        spinner.finish_with_message(format!("OPC UA server started on {}", self.bind_addr));
        Ok(())
    }

    async fn stop_server(&self, _ctx: &mut CliContext) -> CliResult<()> {
        if let Some(server) = self.server.lock().await.as_ref() {
            let _ = server.stop().await;
        }

        if let Some(task) = self.server_task.lock().await.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), task).await;
        }

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
    /// Device tags.
    tags: Tags,
    /// Server instance (for shutdown).
    server: Arc<Mutex<Option<Arc<BACnetServer>>>>,
    /// Server task handle.
    server_task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl BacnetCommand {
    pub fn new() -> Self {
        Self {
            bind_addr: "0.0.0.0:47808".parse().unwrap(),
            device_instance: 1234,
            objects: 100,
            bbmd_enabled: false,
            tags: Tags::new(),
            server: Arc::new(Mutex::new(None)),
            server_task: Arc::new(Mutex::new(None)),
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

    pub fn with_tags(mut self, tags: Tags) -> Self {
        self.tags = tags;
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
        let format = ctx.output().format();
        let is_quiet = ctx.is_quiet();
        let is_verbose = ctx.is_verbose();
        let is_debug = ctx.is_debug();

        if !is_quiet {
            if matches!(format, OutputFormat::Table) {
                let output = ctx.output();
                output.header("BACnet/IP Simulator");
                output.kv("Bind Address", self.bind_addr);
                output.kv("Device Instance", self.device_instance);
                output.kv("Objects", self.objects);
                output.kv("BBMD", if self.bbmd_enabled { "Enabled" } else { "Disabled" });
            }
        }

        // Verbose: show extra details
        if is_verbose {
            let per_type = self.objects / 4;
            ctx.vprintln(format!("  Objects per Type: {} (AI: {}, AO: {}, BI: {}, BO: {})", per_type, per_type, per_type, per_type, per_type));
            ctx.vprintln(format!("  Device Name: Mabinogion BACnet Simulator"));
        }

        // Debug: dump full configuration
        if is_debug {
            ctx.dprintln(format!("Bind address: {}", self.bind_addr));
            ctx.dprintln(format!("Device instance: {}", self.device_instance));
            ctx.dprintln(format!("Total objects: {}, BBMD: {}", self.objects, self.bbmd_enabled));
        }

        self.start_server(ctx).await?;

        let per_type = self.objects / 4;

        if !is_quiet {
            match format {
                OutputFormat::Table => {
                    let colors_enabled = ctx.colors_enabled();
                    let table = TableBuilder::new(colors_enabled)
                        .header(["Object Type", "Count", "Status"])
                        .status_row(["Device", "1", "Online"], StatusType::Success)
                        .status_row(["Analog Input", &per_type.to_string(), "Active"], StatusType::Success)
                        .status_row(["Analog Output", &per_type.to_string(), "Active"], StatusType::Success)
                        .status_row(["Binary Input", &per_type.to_string(), "Active"], StatusType::Success)
                        .status_row(["Binary Output", &per_type.to_string(), "Active"], StatusType::Success);
                    table.print();
                }
                _ => {
                    #[derive(Serialize)]
                    struct BacnetServerInfo {
                        protocol: String,
                        bind_address: String,
                        device_instance: u32,
                        objects: usize,
                        bbmd_enabled: bool,
                        object_types: Vec<ObjectTypeInfo>,
                        status: String,
                    }
                    #[derive(Serialize)]
                    struct ObjectTypeInfo {
                        object_type: String,
                        count: usize,
                        status: String,
                    }
                    let info = BacnetServerInfo {
                        protocol: "BACnet/IP".into(),
                        bind_address: self.bind_addr.to_string(),
                        device_instance: self.device_instance,
                        objects: self.objects,
                        bbmd_enabled: self.bbmd_enabled,
                        object_types: vec![
                            ObjectTypeInfo { object_type: "Device".into(), count: 1, status: "Online".into() },
                            ObjectTypeInfo { object_type: "Analog Input".into(), count: per_type, status: "Active".into() },
                            ObjectTypeInfo { object_type: "Analog Output".into(), count: per_type, status: "Active".into() },
                            ObjectTypeInfo { object_type: "Binary Input".into(), count: per_type, status: "Active".into() },
                            ObjectTypeInfo { object_type: "Binary Output".into(), count: per_type, status: "Active".into() },
                        ],
                        status: "Online".into(),
                    };
                    let _ = ctx.output().write(&info);
                }
            }
        }

        if !is_quiet {
            ctx.output().info("Press Ctrl+C to stop");
        }
        ctx.shutdown_signal().notified().await;

        self.stop_server(ctx).await?;
        if !is_quiet {
            ctx.output().success("BACnet simulator stopped");
        }

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

        let config = BacnetServerConfig::new(self.device_instance)
            .with_bind_addr(self.bind_addr)
            .with_device_name("Mabinogion BACnet Simulator");

        // Create object registry with sample objects
        let registry = ObjectRegistry::new();

        let objects_per_type = self.objects / 4;
        for i in 0..objects_per_type {
            let ai = AnalogInput::new((i + 1) as u32, format!("AI_{}", i + 1));
            registry.register(Arc::new(ai));
        }
        for i in 0..objects_per_type {
            let ao = AnalogOutput::new((i + 1) as u32, format!("AO_{}", i + 1));
            registry.register(Arc::new(ao));
        }
        for i in 0..objects_per_type {
            let bi = BinaryInput::new((i + 1) as u32, format!("BI_{}", i + 1));
            registry.register(Arc::new(bi));
        }
        for i in 0..objects_per_type {
            let bo = BinaryOutput::new((i + 1) as u32, format!("BO_{}", i + 1));
            registry.register(Arc::new(bo));
        }

        let server = Arc::new(BACnetServer::new(config, registry));

        {
            let mut server_guard = self.server.lock().await;
            *server_guard = Some(server.clone());
        }

        let server_clone = server.clone();
        let task = tokio::spawn(async move {
            if let Err(e) = server_clone.run().await {
                tracing::error!("BACnet server error: {}", e);
            }
        });

        {
            let mut task_guard = self.server_task.lock().await;
            *task_guard = Some(task);
        }

        tokio::time::sleep(Duration::from_millis(100)).await;

        spinner.finish_with_message(format!("BACnet server started on {}", self.bind_addr));
        Ok(())
    }

    async fn stop_server(&self, _ctx: &mut CliContext) -> CliResult<()> {
        if let Some(server) = self.server.lock().await.as_ref() {
            server.shutdown();
        }

        if let Some(task) = self.server_task.lock().await.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), task).await;
        }

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
    /// Device tags.
    tags: Tags,
    /// Server instance (for shutdown).
    server: Arc<Mutex<Option<Arc<KnxServer>>>>,
    /// Server task handle.
    server_task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl KnxCommand {
    pub fn new() -> Self {
        Self {
            bind_addr: "0.0.0.0:3671".parse().unwrap(),
            individual_address: "1.1.1".into(),
            group_objects: 100,
            tags: Tags::new(),
            server: Arc::new(Mutex::new(None)),
            server_task: Arc::new(Mutex::new(None)),
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

    pub fn with_tags(mut self, tags: Tags) -> Self {
        self.tags = tags;
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
        let format = ctx.output().format();
        let is_quiet = ctx.is_quiet();
        let is_verbose = ctx.is_verbose();
        let is_debug = ctx.is_debug();

        if !is_quiet {
            if matches!(format, OutputFormat::Table) {
                let output = ctx.output();
                output.header("KNXnet/IP Simulator");
                output.kv("Bind Address", self.bind_addr);
                output.kv("Individual Address", &self.individual_address);
                output.kv("Group Objects", self.group_objects);
            }
        }

        // Verbose: show extra details
        if is_verbose {
            ctx.vprintln(format!("  Max Connections: 10"));
            ctx.vprintln(format!("  Services: Core, Device Management, Tunneling"));
        }

        // Debug: dump full configuration
        if is_debug {
            ctx.dprintln(format!("Bind address: {}", self.bind_addr));
            ctx.dprintln(format!("Individual address: {}", self.individual_address));
            ctx.dprintln(format!("Group objects: {}", self.group_objects));
        }

        self.start_server(ctx).await?;

        if !is_quiet {
            match format {
                OutputFormat::Table => {
                    let colors_enabled = ctx.colors_enabled();
                    let table = TableBuilder::new(colors_enabled)
                        .header(["Service", "Status"])
                        .status_row(["Core", "Ready"], StatusType::Success)
                        .status_row(["Device Management", "Ready"], StatusType::Success)
                        .status_row(["Tunneling", "Ready"], StatusType::Success);
                    table.print();
                }
                _ => {
                    #[derive(Serialize)]
                    struct KnxServerInfo {
                        protocol: String,
                        bind_address: String,
                        individual_address: String,
                        group_objects: usize,
                        services: Vec<ServiceInfo>,
                        status: String,
                    }
                    #[derive(Serialize)]
                    struct ServiceInfo {
                        service: String,
                        status: String,
                    }
                    let info = KnxServerInfo {
                        protocol: "KNXnet/IP".into(),
                        bind_address: self.bind_addr.to_string(),
                        individual_address: self.individual_address.clone(),
                        group_objects: self.group_objects,
                        services: vec![
                            ServiceInfo { service: "Core".into(), status: "Ready".into() },
                            ServiceInfo { service: "Device Management".into(), status: "Ready".into() },
                            ServiceInfo { service: "Tunneling".into(), status: "Ready".into() },
                        ],
                        status: "Online".into(),
                    };
                    let _ = ctx.output().write(&info);
                }
            }
        }

        if !is_quiet {
            ctx.output().info("Press Ctrl+C to stop");
        }
        ctx.shutdown_signal().notified().await;

        self.stop_server(ctx).await?;
        if !is_quiet {
            ctx.output().success("KNX simulator stopped");
        }

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

        // Parse individual address
        let individual_address: IndividualAddress = self.individual_address.parse()
            .map_err(|_| crate::error::CliError::ExecutionFailed {
                message: format!("Invalid individual address: {}", self.individual_address)
            })?;

        let config = KnxServerConfig {
            bind_addr: self.bind_addr,
            individual_address,
            max_connections: 256,
            ..Default::default()
        };

        // Create group objects based on --groups parameter
        let group_table = Arc::new(GroupObjectTable::new());
        let dpt_types = [
            DptId::new(1, 1),   // Switch (bool)
            DptId::new(5, 1),   // Scaling (0-100%)
            DptId::new(9, 1),   // Temperature (float16)
            DptId::new(9, 4),   // Lux
            DptId::new(9, 7),   // Humidity
            DptId::new(12, 1),  // Counter (u32)
            DptId::new(13, 1),  // Counter signed (i32)
            DptId::new(14, 56), // Float (f32)
        ];
        let dpt_names = [
            "Switch", "Scaling", "Temperature", "Lux",
            "Humidity", "Counter", "SignedCounter", "Float",
        ];

        for i in 0..self.group_objects {
            let main = ((i / 256) + 1) as u8;
            let middle = ((i / 8) % 8) as u8;
            let sub = (i % 256) as u8;
            let addr = GroupAddress::three_level(main, middle, sub);
            let dpt_idx = i % dpt_types.len();
            let name = format!("{}_{}", dpt_names[dpt_idx], i);
            if let Err(e) = group_table.create(addr, &name, &dpt_types[dpt_idx]) {
                tracing::warn!("Failed to create group object {}: {}", i, e);
            }
        }

        let server = Arc::new(KnxServer::new(config).with_group_objects(group_table));

        {
            let mut server_guard = self.server.lock().await;
            *server_guard = Some(server.clone());
        }

        let server_clone = server.clone();
        let task = tokio::spawn(async move {
            if let Err(e) = server_clone.start().await {
                tracing::error!("KNX server error: {}", e);
            }
        });

        {
            let mut task_guard = self.server_task.lock().await;
            *task_guard = Some(task);
        }

        tokio::time::sleep(Duration::from_millis(100)).await;

        spinner.finish_with_message(format!("KNX server started on {}", self.bind_addr));
        Ok(())
    }

    async fn stop_server(&self, _ctx: &mut CliContext) -> CliResult<()> {
        // Take server out to call stop (KnxServer::stop has Send issues with parking_lot)
        let server_opt = self.server.lock().await.take();
        if let Some(server) = server_opt {
            // Use spawn_blocking to handle the non-Send future
            let _ = tokio::task::spawn_blocking(move || {
                let rt = tokio::runtime::Handle::current();
                rt.block_on(async {
                    let _ = server.stop().await;
                })
            }).await;
        }

        if let Some(task) = self.server_task.lock().await.take() {
            let _ = tokio::time::timeout(Duration::from_secs(5), task).await;
        }

        Ok(())
    }
}
