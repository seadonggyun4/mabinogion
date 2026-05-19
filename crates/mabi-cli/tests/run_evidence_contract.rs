use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use mabi_runtime::{
    PassCriteriaEvidence, ProtocolProfileEvidence, RunEvidenceBuilder, RuntimeSessionSnapshot,
    ServiceSnapshot, RUN_EVIDENCE_SCHEMA_VERSION, TRIAL_ARTIFACT_CONTRACT_VERSION,
};

#[derive(Debug, Deserialize)]
struct RunEvidenceContract {
    schema_version: u16,
    contract_version: String,
    trial_artifact_contract_version: String,
    required_fields: Vec<String>,
    version_fields: Vec<String>,
    embedded_runtime_fields: Vec<String>,
    optional_report_metrics: Vec<String>,
    public_private_policy: PublicPrivatePolicy,
}

#[derive(Debug, Deserialize)]
struct PublicPrivatePolicy {
    public_summary_may_include: Vec<String>,
    private_only: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ArtifactContract {
    schema_version: u16,
    contract_version: String,
    artifact_metadata_fields: ArtifactMetadataFields,
    visibility_values: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ArtifactMetadataFields {
    required: Vec<String>,
    optional: Vec<String>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root should resolve")
}

fn read_yaml<T: for<'de> Deserialize<'de>>(path: impl AsRef<Path>) -> T {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_yaml::from_str(&contents)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn mabi_version_json() -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_mabi"))
        .args(["--format", "json", "version"])
        .output()
        .expect("mabi version should run");
    assert!(
        output.status.success(),
        "version failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("version output should be JSON")
}

#[test]
fn run_evidence_contract_yaml_declares_required_fields() {
    let root = repo_root();
    let contract: RunEvidenceContract =
        read_yaml(root.join("docs/evidence/run-evidence-schema.yaml"));

    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.contract_version, RUN_EVIDENCE_SCHEMA_VERSION);
    assert_eq!(
        contract.trial_artifact_contract_version,
        TRIAL_ARTIFACT_CONTRACT_VERSION
    );
    for field in [
        "run_id",
        "engine_version",
        "protocol_profile",
        "trial_suite_version",
        "started_at",
        "ended_at",
        "feature_flags",
        "pass_criteria",
        "failure_replay_artifacts",
        "public_private_boundary",
    ] {
        assert!(
            contract.required_fields.iter().any(|entry| entry == field),
            "Run Evidence required field {field} should be documented"
        );
    }
    for field in [
        "run_evidence_schema_version",
        "trial_artifact_contract_version",
        "runtime_contract_version",
        "snapshot_metadata_version",
    ] {
        assert!(
            contract.version_fields.iter().any(|entry| entry == field),
            "Run Evidence version field {field} should be documented"
        );
    }
    assert!(contract
        .embedded_runtime_fields
        .iter()
        .any(|entry| entry == "runtime_snapshot.services.metadata._runtime"));
    assert!(contract
        .optional_report_metrics
        .iter()
        .any(|entry| entry == "recovery_events"));
    assert!(contract
        .public_private_policy
        .public_summary_may_include
        .iter()
        .any(|entry| entry == "public failure replay summaries"));
    assert!(contract
        .public_private_policy
        .private_only
        .iter()
        .any(|entry| entry == "raw log paths"));
}

#[test]
fn trial_artifact_contract_yaml_declares_metadata_and_visibility() {
    let root = repo_root();
    let contract: ArtifactContract =
        read_yaml(root.join("docs/evidence/trial-artifact-contract.yaml"));

    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.contract_version, TRIAL_ARTIFACT_CONTRACT_VERSION);
    for field in ["artifact_id", "kind", "visibility"] {
        assert!(
            contract
                .artifact_metadata_fields
                .required
                .iter()
                .any(|entry| entry == field),
            "artifact metadata field {field} should be required"
        );
    }
    for field in ["path", "media_type", "digest", "description"] {
        assert!(
            contract
                .artifact_metadata_fields
                .optional
                .iter()
                .any(|entry| entry == field),
            "artifact metadata field {field} should be optional"
        );
    }
    let visibilities = contract
        .visibility_values
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    assert_eq!(
        visibilities,
        HashSet::from(["public_summary", "private_raw", "internal_only"])
    );
}

#[test]
fn sample_run_evidence_json_contains_contract_required_fields() {
    let root = repo_root();
    let path = root.join("docs/evidence/sample-run-evidence.json");
    let sample: Value = serde_json::from_str(
        &fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display())),
    )
    .expect("sample evidence JSON should parse");
    let contract: RunEvidenceContract =
        read_yaml(root.join("docs/evidence/run-evidence-schema.yaml"));

    for field in contract
        .required_fields
        .iter()
        .chain(contract.version_fields.iter())
    {
        assert!(
            sample.get(field).is_some(),
            "sample evidence should include {field}"
        );
    }
    assert_eq!(
        sample["run_evidence_schema_version"],
        RUN_EVIDENCE_SCHEMA_VERSION
    );
    assert_eq!(
        sample["trial_artifact_contract_version"],
        TRIAL_ARTIFACT_CONTRACT_VERSION
    );
    assert_eq!(
        sample["runtime_snapshot"]["services"][0]["metadata"]["_runtime"]["contract_version"],
        "runtime-contract-v1"
    );
}

#[test]
fn rust_run_evidence_serialization_matches_contract_required_fields() {
    let mut service = ServiceSnapshot::new("contract-evidence");
    service.ensure_runtime_metadata();
    let snapshot = RuntimeSessionSnapshot::new(vec![service]);
    let evidence = RunEvidenceBuilder::new(
        "run-from-rust",
        "trials-2026.05",
        ProtocolProfileEvidence::new("modbus", "modbus.l1.function_code"),
        PassCriteriaEvidence::new("all required checks pass")
            .with_machine_condition(json!({"kind": "all_required_checks_pass"})),
        snapshot,
    )
    .add_feature_flag("opcua-https-disabled")
    .build();
    let value = serde_json::to_value(&evidence).expect("evidence should serialize");

    let root = repo_root();
    let contract: RunEvidenceContract =
        read_yaml(root.join("docs/evidence/run-evidence-schema.yaml"));
    for field in contract.required_fields {
        assert!(
            value.get(&field).is_some(),
            "Rust RunEvidence serialization should include {field}"
        );
    }
    assert!(value.get("scoring_result").is_none());
}

#[test]
fn version_reports_run_evidence_contract_versions() {
    let envelope = mabi_version_json();
    assert_eq!(
        envelope["data"]["contract_versions"]["run_evidence_schema"],
        RUN_EVIDENCE_SCHEMA_VERSION
    );
    assert_eq!(
        envelope["data"]["contract_versions"]["trial_artifact_contract"],
        TRIAL_ARTIFACT_CONTRACT_VERSION
    );
}
