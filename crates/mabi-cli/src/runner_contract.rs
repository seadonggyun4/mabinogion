//! Machine-readable CLI contracts for local Imugi and Trials runners.

use std::io;

use chrono::{SecondsFormat, Utc};
use serde::Serialize;

use mabi_runtime::{
    ProtocolCatalogEntry, RUNTIME_CONTRACT_VERSION, RUN_EVIDENCE_SCHEMA_VERSION,
    SNAPSHOT_METADATA_VERSION, TRIAL_ARTIFACT_CONTRACT_VERSION,
};

use crate::output::{OutputFormat, OutputWriter};

pub const LOCAL_RUNNER_CONTRACT_VERSION: &str = "local-runner-contract-v1";
pub const CLI_OUTPUT_ENVELOPE_VERSION: &str = "cli-output-envelope-v1";
pub const UNIFIED_READINESS_CONTRACT_VERSION: &str = "unified-readiness-contract-v1";
pub const VERSION_METADATA_CONTRACT_VERSION: &str = "version-metadata-contract-v1";
pub const COMPATIBILITY_MATRIX_VERSION: &str = "compatibility-matrix-v1";
pub const PROTOCOL_READINESS_MATRIX_VERSION: &str = "protocol-readiness-matrix-v1";
pub const RELEASE_CHANNEL: &str = "source-build";
pub const COMPATIBLE_TRIAL_SUITE_RANGE: &str = ">=0.1.0 <1.0.0";
pub const COMPATIBILITY_DECISION_OWNER: &str = "mabinogion-trials-or-imugi";
pub const COMPATIBILITY_MATRIX_DOCUMENT: &str = "docs/release/compatibility-matrix.yaml";
pub const RELEASE_POLICY_DOCUMENT: &str = "docs/release/release-checklist.md";
pub const CHANGELOG_POLICY_DOCUMENT: &str = "docs/release/changelog-policy.md";

#[derive(Debug, Clone, Serialize)]
pub struct CliOutputEnvelope<'a, T: Serialize> {
    pub contract_version: &'static str,
    pub envelope_version: &'static str,
    pub command: &'a str,
    pub status: &'static str,
    pub exit_code: i32,
    pub exit_category: CliExitCategory,
    pub generated_at: String,
    pub engine_version: &'static str,
    pub data: &'a T,
    pub warnings: Vec<String>,
    pub errors: Vec<CliErrorPayload>,
}

impl<'a, T: Serialize> CliOutputEnvelope<'a, T> {
    pub fn success(command: &'a str, data: &'a T) -> Self {
        Self::new(command, 0, data, Vec::new(), Vec::new())
    }

    pub fn failure(
        command: &'a str,
        exit_code: i32,
        data: &'a T,
        errors: Vec<CliErrorPayload>,
    ) -> Self {
        Self::new(command, exit_code, data, Vec::new(), errors)
    }

    pub fn new(
        command: &'a str,
        exit_code: i32,
        data: &'a T,
        warnings: Vec<String>,
        errors: Vec<CliErrorPayload>,
    ) -> Self {
        Self {
            contract_version: LOCAL_RUNNER_CONTRACT_VERSION,
            envelope_version: CLI_OUTPUT_ENVELOPE_VERSION,
            command,
            status: if exit_code == 0 { "success" } else { "failure" },
            exit_code,
            exit_category: CliExitCategory::from_exit_code(exit_code),
            generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            engine_version: mabi_core::RELEASE_VERSION,
            data,
            warnings,
            errors,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CliExitCategory {
    Success,
    InputContractError,
    ValidationFailure,
    RuntimeFailure,
    Timeout,
    Interrupted,
    InternalFailure,
}

impl CliExitCategory {
    pub fn from_exit_code(exit_code: i32) -> Self {
        match exit_code {
            0 => Self::Success,
            2 | 3 | 4 | 5 | 7 | 8 => Self::InputContractError,
            6 => Self::ValidationFailure,
            9 => Self::RuntimeFailure,
            124 => Self::Timeout,
            130 => Self::Interrupted,
            _ => Self::InternalFailure,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CliErrorPayload {
    pub category: CliExitCategory,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl CliErrorPayload {
    pub fn new(exit_code: i32, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            category: CliExitCategory::from_exit_code(exit_code),
            code: code.into(),
            message: message.into(),
            path: None,
        }
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionReport {
    pub engine_version: &'static str,
    pub rust_version: &'static str,
    pub registered_protocols: Vec<VersionProtocol>,
    pub contract_versions: VersionContracts,
    pub feature_flags: Vec<String>,
    pub release_metadata: ReleaseMetadata,
    pub trial_compatible_metadata: TrialCompatibleMetadata,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionProtocol {
    pub key: &'static str,
    pub protocol_key: &'static str,
    pub display_name: &'static str,
    pub default_port: u16,
    pub features: Vec<&'static str>,
    #[serde(flatten)]
    pub capability: ProtocolCapabilityVersion,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProtocolCapabilityVersion {
    pub capability_version: &'static str,
    pub readiness_matrix_version: &'static str,
    pub readiness_contract_version: &'static str,
    pub trial_profile_ids: Vec<&'static str>,
    pub breaking_change_scope: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VersionContracts {
    pub local_runner_contract: &'static str,
    pub cli_output_envelope: &'static str,
    pub runtime_contract: &'static str,
    pub snapshot_metadata_contract: &'static str,
    pub unified_readiness_contract: &'static str,
    pub run_evidence_schema: &'static str,
    pub trial_artifact_contract: &'static str,
    pub version_metadata_contract: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseMetadata {
    pub engine_version: &'static str,
    pub workspace_rust_version: &'static str,
    pub release_channel: &'static str,
    pub compatibility_matrix_version: &'static str,
    pub compatibility_matrix_document: &'static str,
    pub release_policy_document: &'static str,
    pub changelog_policy_document: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrialCompatibleMetadata {
    pub trial_definition_owner: &'static str,
    pub scoring_owner: &'static str,
    pub supported_machine_formats: Vec<&'static str>,
    pub runner_facing_commands: Vec<&'static str>,
    pub future_trial_run_contract_documented: bool,
    #[serde(flatten)]
    pub runner_compatibility: RunnerCompatibilityPolicy,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunnerCompatibilityPolicy {
    pub compatible_trial_suite_range: &'static str,
    pub compatibility_decision_owner: &'static str,
    pub runner_policy_document: &'static str,
}

pub fn is_machine_format(format: OutputFormat) -> bool {
    matches!(
        format,
        OutputFormat::Json | OutputFormat::Yaml | OutputFormat::Compact
    )
}

pub fn write_success<T: Serialize>(
    output: &OutputWriter,
    command: &str,
    data: &T,
) -> io::Result<()> {
    output.write(&CliOutputEnvelope::success(command, data))
}

pub fn write_failure<T: Serialize>(
    output: &OutputWriter,
    command: &str,
    exit_code: i32,
    data: &T,
    errors: Vec<CliErrorPayload>,
) -> io::Result<()> {
    output.write(&CliOutputEnvelope::failure(
        command, exit_code, data, errors,
    ))
}

pub fn version_report(
    rust_version: &'static str,
    protocols: &[ProtocolCatalogEntry],
) -> VersionReport {
    VersionReport {
        engine_version: mabi_core::RELEASE_VERSION,
        rust_version,
        registered_protocols: protocols
            .iter()
            .map(|entry| VersionProtocol {
                key: entry.descriptor.key,
                protocol_key: entry.descriptor.key,
                display_name: entry.descriptor.display_name,
                default_port: entry.descriptor.default_port,
                features: entry.features.clone(),
                capability: protocol_capability_version(entry.descriptor.key),
            })
            .collect(),
        contract_versions: VersionContracts {
            local_runner_contract: LOCAL_RUNNER_CONTRACT_VERSION,
            cli_output_envelope: CLI_OUTPUT_ENVELOPE_VERSION,
            runtime_contract: RUNTIME_CONTRACT_VERSION,
            snapshot_metadata_contract: SNAPSHOT_METADATA_VERSION,
            unified_readiness_contract: UNIFIED_READINESS_CONTRACT_VERSION,
            run_evidence_schema: RUN_EVIDENCE_SCHEMA_VERSION,
            trial_artifact_contract: TRIAL_ARTIFACT_CONTRACT_VERSION,
            version_metadata_contract: VERSION_METADATA_CONTRACT_VERSION,
        },
        feature_flags: feature_flags(),
        release_metadata: ReleaseMetadata {
            engine_version: mabi_core::RELEASE_VERSION,
            workspace_rust_version: rust_version,
            release_channel: RELEASE_CHANNEL,
            compatibility_matrix_version: COMPATIBILITY_MATRIX_VERSION,
            compatibility_matrix_document: COMPATIBILITY_MATRIX_DOCUMENT,
            release_policy_document: RELEASE_POLICY_DOCUMENT,
            changelog_policy_document: CHANGELOG_POLICY_DOCUMENT,
        },
        trial_compatible_metadata: TrialCompatibleMetadata {
            trial_definition_owner: "mabinogion-trials",
            scoring_owner: "mabinogion-trials",
            supported_machine_formats: vec!["json", "yaml", "compact"],
            runner_facing_commands: vec!["doctor", "inspect", "validate", "version"],
            future_trial_run_contract_documented: true,
            runner_compatibility: RunnerCompatibilityPolicy {
                compatible_trial_suite_range: COMPATIBLE_TRIAL_SUITE_RANGE,
                compatibility_decision_owner: COMPATIBILITY_DECISION_OWNER,
                runner_policy_document: RELEASE_POLICY_DOCUMENT,
            },
        },
    }
}

pub fn protocol_capability_version(protocol_key: &str) -> ProtocolCapabilityVersion {
    match protocol_key {
        "modbus" => ProtocolCapabilityVersion {
            capability_version: "modbus-capabilities-v1",
            readiness_matrix_version: PROTOCOL_READINESS_MATRIX_VERSION,
            readiness_contract_version: UNIFIED_READINESS_CONTRACT_VERSION,
            trial_profile_ids: vec![
                "modbus.l1.function_code",
                "modbus.l1.register_map",
                "modbus.l1.exception_response",
                "modbus.l2.multi_unit",
                "modbus.l2.timeout",
                "modbus.l2.partial_response",
                "modbus.l2.slow_device",
            ],
            breaking_change_scope: version_breaking_change_scope(),
        },
        "opcua" => ProtocolCapabilityVersion {
            capability_version: "opcua-capabilities-v1",
            readiness_matrix_version: PROTOCOL_READINESS_MATRIX_VERSION,
            readiness_contract_version: UNIFIED_READINESS_CONTRACT_VERSION,
            trial_profile_ids: vec![
                "opcua.l1.session_lifecycle",
                "opcua.l2.secure_channel_renewal",
                "opcua.l3.reconnect",
                "opcua.l2.subscription",
                "opcua.l3.timeout",
                "opcua.l3.malformed_service_response",
                "opcua.l2.operation_limit",
            ],
            breaking_change_scope: version_breaking_change_scope(),
        },
        "bacnet" => ProtocolCapabilityVersion {
            capability_version: "bacnet-capabilities-v1",
            readiness_matrix_version: PROTOCOL_READINESS_MATRIX_VERSION,
            readiness_contract_version: UNIFIED_READINESS_CONTRACT_VERSION,
            trial_profile_ids: vec![
                "bacnet.l1.discovery",
                "bacnet.l1.property_io",
                "bacnet.l2.cov",
                "bacnet.l3.segmentation",
                "bacnet.l3.bbmd_fdr",
                "bacnet.l3.duplicate_handling",
            ],
            breaking_change_scope: version_breaking_change_scope(),
        },
        "knx" => ProtocolCapabilityVersion {
            capability_version: "knx-capabilities-v1",
            readiness_matrix_version: PROTOCOL_READINESS_MATRIX_VERSION,
            readiness_contract_version: UNIFIED_READINESS_CONTRACT_VERSION,
            trial_profile_ids: vec![
                "knx.l1.discovery",
                "knx.l1.tunneling_lifecycle",
                "knx.l1.group_value_io",
                "knx.l2.dpt_codec",
                "knx.l2.sequence_validation",
                "knx.l2.heartbeat_connection_state",
            ],
            breaking_change_scope: version_breaking_change_scope(),
        },
        _ => ProtocolCapabilityVersion {
            capability_version: "unknown-capabilities-v1",
            readiness_matrix_version: PROTOCOL_READINESS_MATRIX_VERSION,
            readiness_contract_version: UNIFIED_READINESS_CONTRACT_VERSION,
            trial_profile_ids: Vec::new(),
            breaking_change_scope: version_breaking_change_scope(),
        },
    }
}

fn version_breaking_change_scope() -> Vec<&'static str> {
    vec![
        "config",
        "runtime_contract",
        "readiness_contract",
        "run_evidence_schema",
        "version_metadata_contract",
    ]
}

fn feature_flags() -> Vec<String> {
    let mut flags = Vec::new();
    if cfg!(feature = "opcua-https") {
        flags.push("opcua-https".to_string());
    } else {
        flags.push("opcua-https-disabled".to_string());
    }
    flags
}
