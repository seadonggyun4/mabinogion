//! List command implementation.
//!
//! Lists devices, protocols, and other resources.

use crate::context::CliContext;
use crate::error::{CliError, CliResult};
use crate::output::{DeviceSummary, OutputFormat, StatusType, TableBuilder};
use crate::runner::{Command, CommandOutput};
use async_trait::async_trait;
use mabi_core::prelude::*;
use serde::Serialize;

/// Resource type to list.
#[derive(Debug, Clone, Copy, Default)]
pub enum ListResource {
    #[default]
    Devices,
    Protocols,
    Points,
    Scenarios,
}

impl ListResource {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "devices" | "device" | "d" => Some(Self::Devices),
            "protocols" | "protocol" | "p" => Some(Self::Protocols),
            "points" | "point" => Some(Self::Points),
            "scenarios" | "scenario" | "s" => Some(Self::Scenarios),
            _ => None,
        }
    }
}

/// List command for displaying resources.
pub struct ListCommand {
    /// Resource type to list.
    resource: ListResource,
    /// Filter by protocol.
    protocol: Option<Protocol>,
    /// Filter by device ID pattern.
    device_filter: Option<String>,
    /// Maximum number of items to show.
    limit: Option<usize>,
}

impl ListCommand {
    /// Create a new list command.
    pub fn new(resource: ListResource) -> Self {
        Self {
            resource,
            protocol: None,
            device_filter: None,
            limit: None,
        }
    }

    /// Filter by protocol.
    pub fn with_protocol(mut self, protocol: Protocol) -> Self {
        self.protocol = Some(protocol);
        self
    }

    /// Filter by device ID pattern.
    pub fn with_device_filter(mut self, pattern: impl Into<String>) -> Self {
        self.device_filter = Some(pattern.into());
        self
    }

    /// Set the maximum number of items.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// List devices.
    async fn list_devices(&self, ctx: &mut CliContext) -> CliResult<CommandOutput> {
        let output = ctx.output();

        // Get devices from engine if available
        let devices: Vec<DeviceSummary> = {
            let engine_guard = ctx.engine_ref().read().await;
            if let Some(engine) = engine_guard.as_ref() {
                engine
                    .list_devices()
                    .into_iter()
                    .filter(|info| {
                        // Protocol filter
                        if let Some(ref proto) = self.protocol {
                            if info.protocol != *proto {
                                return false;
                            }
                        }
                        // Device ID filter
                        if let Some(ref pattern) = self.device_filter {
                            if !info.id.contains(pattern) {
                                return false;
                            }
                        }
                        true
                    })
                    .take(self.limit.unwrap_or(usize::MAX))
                    .map(|info| {
                        DeviceSummary {
                            id: info.id.clone(),
                            name: info.name.clone(),
                            protocol: format!("{:?}", info.protocol),
                            status: format!("{:?}", info.state),
                            points: info.point_count,
                            last_update: info.updated_at.to_rfc3339(),
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            }
        };

        if matches!(output.format(), OutputFormat::Json | OutputFormat::Yaml) {
            output.write(&devices)?;
            return Ok(CommandOutput::quiet_success());
        }

        output.header("Devices");

        if devices.is_empty() {
            output.info("No devices found");
            if self.protocol.is_some() || self.device_filter.is_some() {
                output.kv("Tip", "Try removing filters to see all devices");
            }
        } else {
            let mut table = TableBuilder::new(output.colors_enabled())
                .header(["ID", "Name", "Protocol", "Points", "Status"]);

            for device in &devices {
                let status_type = match device.status.as_str() {
                    "Online" => StatusType::Success,
                    "Offline" | "Error" => StatusType::Error,
                    _ => StatusType::Neutral,
                };
                table = table.status_row(
                    [
                        &device.id,
                        &device.name,
                        &device.protocol,
                        &device.points.to_string(),
                        &device.status,
                    ],
                    status_type,
                );
            }
            table.print();
            output.kv("Total", devices.len());
        }

        Ok(CommandOutput::quiet_success())
    }

    /// List supported protocols.
    async fn list_protocols(&self, ctx: &mut CliContext) -> CliResult<CommandOutput> {
        let output = ctx.output();

        let protocols = vec![
            ProtocolInfo {
                name: "Modbus TCP".into(),
                protocol: "modbus_tcp".into(),
                default_port: 502,
                description: "Modbus TCP/IP protocol".into(),
                features: vec!["Read/Write Coils", "Read/Write Registers", "Multi-unit support"],
            },
            ProtocolInfo {
                name: "Modbus RTU".into(),
                protocol: "modbus_rtu".into(),
                default_port: 0,
                description: "Modbus RTU serial protocol".into(),
                features: vec!["Serial communication", "CRC validation", "Multi-device bus"],
            },
            ProtocolInfo {
                name: "OPC UA".into(),
                protocol: "opcua".into(),
                default_port: 4840,
                description: "OPC Unified Architecture".into(),
                features: vec!["Subscriptions", "History", "Security", "Address space"],
            },
            ProtocolInfo {
                name: "BACnet/IP".into(),
                protocol: "bacnet".into(),
                default_port: 47808,
                description: "BACnet over IP".into(),
                features: vec!["Read/Write Properties", "COV Subscriptions", "BBMD", "Device discovery"],
            },
            ProtocolInfo {
                name: "KNXnet/IP".into(),
                protocol: "knx".into(),
                default_port: 3671,
                description: "KNX over IP".into(),
                features: vec!["Tunneling", "Group addressing", "DPT support"],
            },
        ];

        if matches!(output.format(), OutputFormat::Json | OutputFormat::Yaml) {
            output.write(&protocols)?;
            return Ok(CommandOutput::quiet_success());
        }

        output.header("Supported Protocols");

        let mut table = TableBuilder::new(output.colors_enabled())
            .header(["Protocol", "Name", "Port", "Features"]);

        for proto in &protocols {
            table = table.row([
                &proto.protocol,
                &proto.name,
                &proto.default_port.to_string(),
                &proto.features.join(", "),
            ]);
        }
        table.print();

        Ok(CommandOutput::quiet_success())
    }

    /// List data points.
    async fn list_points(&self, ctx: &mut CliContext) -> CliResult<CommandOutput> {
        let output = ctx.output();

        // Get points from devices
        // Note: This is a simplified implementation that lists device info
        // Full point listing would require device handles with point_definitions()
        let points: Vec<PointInfo> = {
            let engine_guard = ctx.engine_ref().read().await;
            if let Some(engine) = engine_guard.as_ref() {
                engine
                    .list_devices()
                    .into_iter()
                    .filter(|info| {
                        if let Some(ref pattern) = self.device_filter {
                            info.id.contains(pattern)
                        } else {
                            true
                        }
                    })
                    .flat_map(|info| {
                        // For now, create placeholder points based on point_count
                        // In real implementation, we would use device handles to get actual point definitions
                        (0..info.point_count).map(move |i| PointInfo {
                            device_id: info.id.clone(),
                            point_id: format!("point_{}", i),
                            data_type: "Unknown".into(),
                            access: "ReadWrite".into(),
                            description: String::new(),
                        })
                    })
                    .take(self.limit.unwrap_or(usize::MAX))
                    .collect()
            } else {
                Vec::new()
            }
        };

        if matches!(output.format(), OutputFormat::Json | OutputFormat::Yaml) {
            output.write(&points)?;
            return Ok(CommandOutput::quiet_success());
        }

        output.header("Data Points");

        if points.is_empty() {
            output.info("No data points found");
        } else {
            let mut table = TableBuilder::new(output.colors_enabled())
                .header(["Device", "Point ID", "Type", "Access", "Description"]);

            for point in &points {
                table = table.row([
                    &point.device_id,
                    &point.point_id,
                    &point.data_type,
                    &point.access,
                    &point.description,
                ]);
            }
            table.print();
            output.kv("Total", points.len());
        }

        Ok(CommandOutput::quiet_success())
    }

    /// List scenarios.
    async fn list_scenarios(&self, ctx: &mut CliContext) -> CliResult<CommandOutput> {
        let output = ctx.output();
        let working_dir = ctx.working_dir().clone();

        // Search for scenario files
        let scenarios_dir = working_dir.join("scenarios");
        let mut scenarios = Vec::new();

        if scenarios_dir.exists() {
            let mut entries = tokio::fs::read_dir(&scenarios_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "yaml" || e == "yml") {
                    if let Ok(content) = tokio::fs::read_to_string(&path).await {
                        if let Ok(scenario) = serde_yaml::from_str::<super::run::ScenarioConfig>(&content) {
                            scenarios.push(ScenarioInfo {
                                name: scenario.name,
                                file: path.file_name().unwrap_or_default().to_string_lossy().into(),
                                devices: scenario.devices.len(),
                                events: scenario.events.len(),
                                description: scenario.description,
                            });
                        }
                    }
                }

                if let Some(limit) = self.limit {
                    if scenarios.len() >= limit {
                        break;
                    }
                }
            }
        }

        if matches!(output.format(), OutputFormat::Json | OutputFormat::Yaml) {
            output.write(&scenarios)?;
            return Ok(CommandOutput::quiet_success());
        }

        output.header("Scenarios");

        if scenarios.is_empty() {
            output.info("No scenarios found");
            output.kv("Search path", scenarios_dir.display());
        } else {
            let mut table = TableBuilder::new(output.colors_enabled())
                .header(["Name", "File", "Devices", "Events"]);

            for scenario in &scenarios {
                table = table.row([
                    &scenario.name,
                    &scenario.file,
                    &scenario.devices.to_string(),
                    &scenario.events.to_string(),
                ]);
            }
            table.print();
            output.kv("Total", scenarios.len());
        }

        Ok(CommandOutput::quiet_success())
    }
}

#[async_trait]
impl Command for ListCommand {
    fn name(&self) -> &str {
        "list"
    }

    fn description(&self) -> &str {
        "List devices, protocols, points, or scenarios"
    }

    async fn execute(&self, ctx: &mut CliContext) -> CliResult<CommandOutput> {
        match self.resource {
            ListResource::Devices => self.list_devices(ctx).await,
            ListResource::Protocols => self.list_protocols(ctx).await,
            ListResource::Points => self.list_points(ctx).await,
            ListResource::Scenarios => self.list_scenarios(ctx).await,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ProtocolInfo {
    name: String,
    protocol: String,
    default_port: u16,
    description: String,
    features: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
struct PointInfo {
    device_id: String,
    point_id: String,
    data_type: String,
    access: String,
    description: String,
}

#[derive(Debug, Clone, Serialize)]
struct ScenarioInfo {
    name: String,
    file: String,
    devices: usize,
    events: usize,
    description: String,
}
