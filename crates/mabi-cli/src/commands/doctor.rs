//! Self-contained installation diagnostics for the packaged CLI.

use std::collections::BTreeSet;
use std::net::{SocketAddr, TcpListener};

use async_trait::async_trait;
use serde::Serialize;
use serde_json::json;
use tokio::time::Duration;

use mabi_runtime::{ProtocolLaunchSpec, RuntimeExtensions, RuntimeSession, RuntimeSessionSpec};

use crate::context::CliContext;
use crate::error::CliResult;
use crate::output::{OutputFormat, StatusType, TableBuilder};
use crate::runner::{Command, CommandOutput};
use crate::runner_contract::{is_machine_format, write_failure, write_success, CliErrorPayload};
use crate::runtime_registry::{protocol_catalog, workspace_protocol_registry};

/// Protocol selection for `mabi doctor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorProtocol {
    All,
    Modbus,
    Opcua,
    Bacnet,
    Knx,
}

impl DoctorProtocol {
    fn selected_keys(self) -> &'static [&'static str] {
        match self {
            Self::All => &["modbus", "opcua", "bacnet", "knx"],
            Self::Modbus => &["modbus"],
            Self::Opcua => &["opcua"],
            Self::Bacnet => &["bacnet"],
            Self::Knx => &["knx"],
        }
    }
}

/// Doctor command that validates the installed binary without external tools.
#[derive(Debug, Clone)]
pub struct DoctorCommand {
    protocol: DoctorProtocol,
    readiness_timeout: Duration,
}

impl DoctorCommand {
    pub fn new(protocol: DoctorProtocol, readiness_timeout: Duration) -> Self {
        Self {
            protocol,
            readiness_timeout,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub version: String,
    pub checks: Vec<DoctorCheck>,
    pub protocols: Vec<ProtocolDoctorResult>,
    pub optional_prereqs: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorCheck {
    pub id: String,
    pub status: DoctorStatus,
    pub message: String,
}

impl DoctorCheck {
    fn pass(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: DoctorStatus::Pass,
            message: message.into(),
        }
    }

    fn fail(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: DoctorStatus::Fail,
            message: message.into(),
        }
    }

    fn skip(id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status: DoctorStatus::Skip,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Pass,
    Fail,
    Skip,
}

impl DoctorStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Skip => "skip",
        }
    }

    fn table_status(self) -> StatusType {
        match self {
            Self::Pass => StatusType::Success,
            Self::Fail => StatusType::Error,
            Self::Skip => StatusType::Warning,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProtocolDoctorResult {
    pub protocol: String,
    pub launch_ok: bool,
    pub ready_ok: bool,
    pub snapshot_ok: bool,
    pub stop_ok: bool,
    pub metadata_keys: Vec<String>,
    pub message: String,
}

impl ProtocolDoctorResult {
    fn status(&self) -> DoctorStatus {
        if self.launch_ok && self.ready_ok && self.snapshot_ok && self.stop_ok {
            DoctorStatus::Pass
        } else {
            DoctorStatus::Fail
        }
    }
}

#[async_trait]
impl Command for DoctorCommand {
    fn name(&self) -> &str {
        "doctor"
    }

    fn description(&self) -> &str {
        "Run self-contained installation diagnostics"
    }

    async fn execute(&self, ctx: &mut CliContext) -> CliResult<CommandOutput> {
        let report = self.run_report().await;
        let has_failures = report_has_failures(&report);
        let exit_code = if has_failures { 1 } else { 0 };
        render_report(ctx, &report, exit_code)?;

        if has_failures {
            if is_machine_format(ctx.output().format()) {
                Ok(CommandOutput::quiet_failure(exit_code))
            } else {
                Ok(CommandOutput::failure(
                    exit_code,
                    "mabi doctor found failures",
                ))
            }
        } else {
            Ok(CommandOutput::quiet_success())
        }
    }
}

impl DoctorCommand {
    async fn run_report(&self) -> DoctorReport {
        let mut checks = Vec::new();
        checks.push(DoctorCheck::pass(
            "version.release",
            format!(
                "CLI release version is {}",
                mabi_core::version::RELEASE_VERSION
            ),
        ));

        let catalog = protocol_catalog();
        let registered: BTreeSet<&str> = catalog.iter().map(|entry| entry.descriptor.key).collect();
        for expected in ["modbus", "opcua", "bacnet", "knx"] {
            if registered.contains(expected) {
                checks.push(DoctorCheck::pass(
                    format!("registry.{}", expected),
                    format!("{} protocol driver is registered", expected),
                ));
            } else {
                checks.push(DoctorCheck::fail(
                    format!("registry.{}", expected),
                    format!("{} protocol driver is missing", expected),
                ));
            }
        }

        let mut protocols = Vec::new();
        for protocol in self.protocol.selected_keys() {
            protocols.push(self.run_protocol_smoke(protocol).await);
        }

        DoctorReport {
            version: mabi_core::version::RELEASE_VERSION.to_string(),
            checks,
            protocols,
            optional_prereqs: optional_prereqs(),
        }
    }

    async fn run_protocol_smoke(&self, protocol: &str) -> ProtocolDoctorResult {
        let launch = doctor_launch_spec(protocol);
        let mut result = ProtocolDoctorResult {
            protocol: protocol.to_string(),
            launch_ok: false,
            ready_ok: false,
            snapshot_ok: false,
            stop_ok: false,
            metadata_keys: Vec::new(),
            message: "not started".to_string(),
        };

        let Some(launch) = launch else {
            result.message = "unknown doctor protocol".to_string();
            return result;
        };

        let registry = workspace_protocol_registry();
        let session = RuntimeSession::new(
            RuntimeSessionSpec {
                services: vec![launch],
                readiness_timeout: Some(self.readiness_timeout.as_millis() as u64),
            },
            &registry,
            RuntimeExtensions::default(),
        )
        .await;

        let session = match session {
            Ok(session) => {
                result.launch_ok = true;
                session
            }
            Err(error) => {
                result.message = format!("launch failed: {}", error);
                return result;
            }
        };

        if let Err(error) = session.start(self.readiness_timeout).await {
            let detail = session
                .snapshots()
                .await
                .ok()
                .and_then(|mut snapshots| snapshots.pop())
                .and_then(|snapshot| snapshot.status.last_error);
            result.message = match detail {
                Some(detail) => format!("readiness failed: {}; {}", error, detail),
                None => format!("readiness failed: {}", error),
            };
            let _ = session.stop().await;
            return result;
        }
        result.ready_ok = true;

        match session.snapshots().await {
            Ok(snapshots) => {
                if let Some(snapshot) = snapshots.into_iter().next() {
                    result.metadata_keys = snapshot.metadata.keys().cloned().collect();
                    result.snapshot_ok = protocol_metadata_ok(protocol, &result.metadata_keys);
                    if !result.snapshot_ok {
                        result.message =
                            format!("snapshot missing required metadata for {}", protocol);
                    }
                } else {
                    result.message = "runtime returned no snapshots".to_string();
                }
            }
            Err(error) => {
                result.message = format!("snapshot failed: {}", error);
            }
        }

        match session.stop().await {
            Ok(()) => {
                result.stop_ok = true;
            }
            Err(error) => {
                result.message = format!("stop failed: {}", error);
            }
        }

        if result.status() == DoctorStatus::Pass {
            result.message = "self-contained runtime smoke passed".to_string();
        }
        result
    }
}

fn report_has_failures(report: &DoctorReport) -> bool {
    report
        .checks
        .iter()
        .chain(report.optional_prereqs.iter())
        .any(|check| check.status == DoctorStatus::Fail)
        || report
            .protocols
            .iter()
            .any(|protocol| protocol.status() == DoctorStatus::Fail)
}

fn render_report(ctx: &CliContext, report: &DoctorReport, exit_code: i32) -> CliResult<()> {
    match ctx.output().format() {
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact => {
            if exit_code == 0 {
                write_success(ctx.output(), "doctor", report)?;
            } else {
                write_failure(
                    ctx.output(),
                    "doctor",
                    exit_code,
                    report,
                    vec![CliErrorPayload::new(
                        exit_code,
                        "doctor_failed",
                        "mabi doctor found failures",
                    )],
                )?;
            }
        }
        OutputFormat::Table => {
            ctx.output().header("mabi doctor");
            ctx.output().kv("Version", &report.version);

            TableBuilder::new(ctx.colors_enabled())
                .header(["Check", "Status", "Message"])
                .status_row(
                    [
                        "version".to_string(),
                        "pass".to_string(),
                        format!("mabi {}", report.version),
                    ],
                    StatusType::Success,
                )
                .print();

            let mut registry_table =
                TableBuilder::new(ctx.colors_enabled()).header(["Check", "Status", "Message"]);
            for check in &report.checks {
                registry_table = registry_table.status_row(
                    [
                        check.id.clone(),
                        check.status.as_str().to_string(),
                        check.message.clone(),
                    ],
                    check.status.table_status(),
                );
            }
            registry_table.print();

            let mut protocol_table = TableBuilder::new(ctx.colors_enabled())
                .header(["Protocol", "Launch", "Ready", "Snapshot", "Stop", "Status"]);
            for protocol in &report.protocols {
                protocol_table = protocol_table.status_row(
                    [
                        protocol.protocol.clone(),
                        bool_status(protocol.launch_ok),
                        bool_status(protocol.ready_ok),
                        bool_status(protocol.snapshot_ok),
                        bool_status(protocol.stop_ok),
                        protocol.status().as_str().to_string(),
                    ],
                    protocol.status().table_status(),
                );
            }
            protocol_table.print();

            let mut optional_table =
                TableBuilder::new(ctx.colors_enabled()).header(["Optional", "Status", "Message"]);
            for check in &report.optional_prereqs {
                optional_table = optional_table.status_row(
                    [
                        check.id.clone(),
                        check.status.as_str().to_string(),
                        check.message.clone(),
                    ],
                    check.status.table_status(),
                );
            }
            optional_table.print();
        }
    }
    Ok(())
}

fn bool_status(value: bool) -> String {
    if value { "pass" } else { "fail" }.to_string()
}

fn optional_prereqs() -> Vec<DoctorCheck> {
    [
        (
            "interop.docker",
            "Docker/Compose is only required for source-tree interop matrices",
        ),
        (
            "interop.python",
            "Python peers such as XKNX/BACpypes are optional interop assets",
        ),
        (
            "interop.java",
            "Java peers such as Calimero/Milo are optional interop assets",
        ),
        (
            "interop.node",
            "Node peers such as knx are optional interop assets",
        ),
        (
            "interop.knxd",
            "knxd is optional and never required by installed CLI smoke checks",
        ),
    ]
    .into_iter()
    .map(|(id, message)| DoctorCheck::skip(id, message))
    .collect()
}

fn doctor_launch_spec(protocol: &str) -> Option<ProtocolLaunchSpec> {
    let modbus_bind_addr = reserve_loopback_tcp_addr()
        .map(|address| address.to_string())
        .unwrap_or_else(|| "127.0.0.1:0".to_string());
    let config = match protocol {
        "modbus" => json!({
            "transport": {
                "kind": "tcp",
                "bind_addr": modbus_bind_addr,
                "performance_preset": "default",
            },
            "devices": 1,
            "points_per_device": 4,
        }),
        "opcua" => json!({
            "bind_addr": "127.0.0.1:0",
            "endpoint_path": "/mabi/doctor",
            "nodes": 4,
            "security_mode": "None",
        }),
        "bacnet" => json!({
            "bind_addr": "127.0.0.1:0",
            "device_instance": 9_001,
            "objects": 8,
            "bbmd_enabled": false,
        }),
        "knx" => json!({
            "bind_addr": "127.0.0.1:0",
            "individual_address": "1.1.1",
            "group_objects": 8,
        }),
        _ => return None,
    };

    Some(ProtocolLaunchSpec {
        protocol: protocol.to_string(),
        name: Some(format!("doctor-{}", protocol)),
        config,
    })
}

fn reserve_loopback_tcp_addr() -> Option<SocketAddr> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let address = listener.local_addr().ok()?;
    drop(listener);
    Some(address)
}

fn protocol_metadata_ok(protocol: &str, keys: &[String]) -> bool {
    let keys: BTreeSet<&str> = keys.iter().map(String::as_str).collect();
    let required: &[&str] = match protocol {
        "modbus" => &["_runtime", "transport", "devices", "points", "bind_address"],
        "opcua" => &[
            "_runtime",
            "endpoint",
            "transport_protocol",
            "nodes",
            "security_profile",
        ],
        "bacnet" => &[
            "_runtime",
            "bind_address",
            "device_instance",
            "objects",
            "metrics",
        ],
        "knx" => &[
            "_runtime",
            "bind_address",
            "individual_address",
            "group_objects",
            "metrics",
        ],
        _ => return false,
    };
    required.iter().all(|key| keys.contains(key))
}
