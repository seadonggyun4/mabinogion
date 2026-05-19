use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Deserialize)]
struct VersionMetadataContract {
    contract_version: String,
    required_version_data_fields: Vec<String>,
    required_contract_versions: Vec<String>,
    release_metadata_required_fields: Vec<String>,
    registered_protocol_required_fields: Vec<String>,
    trial_compatible_metadata_required_fields: Vec<String>,
    compatibility_policy: CompatibilityPolicy,
}

#[derive(Debug, Deserialize)]
struct CompatibilityPolicy {
    decision_owner: String,
    mabinogion_role: String,
}

#[derive(Debug, Deserialize)]
struct CompatibilityMatrix {
    matrix_version: String,
    current_engine_version: String,
    workspace_rust_version: String,
    release_channel: String,
    version_metadata_contract: String,
    readiness_matrix_version: String,
    compatible_trial_suite_range: String,
    runner_policy_document: String,
    release_policy_document: String,
    changelog_policy_document: String,
    compatibility_decision_owner: String,
    supported_contract_versions: Vec<String>,
    protocol_capabilities: Vec<ProtocolCapability>,
    ownership_boundary: OwnershipBoundary,
}

#[derive(Debug, Deserialize)]
struct ProtocolCapability {
    protocol_key: String,
    capability_version: String,
    readiness_matrix_version: String,
    readiness_contract_version: String,
    trial_profile_ids: Vec<String>,
    breaking_change_scope: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OwnershipBoundary {
    mabinogion_owns: Vec<String>,
    not_owned_by_mabinogion: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ReadinessMatrix {
    matrix_version: String,
    protocols: Vec<ReadinessProtocol>,
}

#[derive(Debug, Deserialize)]
struct ReadinessProtocol {
    id: String,
    profiles: Vec<ReadinessProfile>,
}

#[derive(Debug, Deserialize)]
struct ReadinessProfile {
    trial_profile_id: String,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root should resolve")
}

fn read_yaml<T>(root: &Path, relative_path: &str) -> T
where
    T: for<'de> Deserialize<'de>,
{
    let path = root.join(relative_path);
    let contents = fs::read_to_string(&path)
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
        "mabi version failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "version stdout should be JSON: {error}\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn string_set(values: &[String]) -> HashSet<&str> {
    values.iter().map(String::as_str).collect()
}

#[test]
fn version_metadata_contract_docs_define_required_release_fields() {
    let root = repo_root();
    let contract: VersionMetadataContract =
        read_yaml(&root, "docs/release/version-metadata-contract.yaml");

    assert_eq!(contract.contract_version, "version-metadata-contract-v1");
    for field in [
        "engine_version",
        "rust_version",
        "registered_protocols",
        "contract_versions",
        "feature_flags",
        "trial_compatible_metadata",
        "release_metadata",
    ] {
        assert!(
            contract
                .required_version_data_fields
                .iter()
                .any(|documented| documented == field),
            "version data should require {field}"
        );
    }
    for field in [
        "engine_version",
        "workspace_rust_version",
        "release_channel",
        "compatibility_matrix_version",
        "compatibility_matrix_document",
        "release_policy_document",
        "changelog_policy_document",
    ] {
        assert!(
            contract
                .release_metadata_required_fields
                .iter()
                .any(|documented| documented == field),
            "release metadata should require {field}"
        );
    }
    for field in [
        "protocol_key",
        "capability_version",
        "readiness_matrix_version",
        "readiness_contract_version",
        "trial_profile_ids",
        "breaking_change_scope",
    ] {
        assert!(
            contract
                .registered_protocol_required_fields
                .iter()
                .any(|documented| documented == field),
            "registered protocol metadata should require {field}"
        );
    }
    for field in [
        "compatible_trial_suite_range",
        "compatibility_decision_owner",
        "runner_policy_document",
    ] {
        assert!(
            contract
                .trial_compatible_metadata_required_fields
                .iter()
                .any(|documented| documented == field),
            "trial compatibility metadata should require {field}"
        );
    }
    assert_eq!(
        contract.compatibility_policy.decision_owner,
        "mabinogion-trials-or-forge"
    );
    assert!(
        contract
            .compatibility_policy
            .mabinogion_role
            .contains("metadata"),
        "mabinogion role should stay limited to metadata export"
    );
}

#[test]
fn compatibility_matrix_matches_workspace_version_and_contract_versions() {
    let root = repo_root();
    let contract: VersionMetadataContract =
        read_yaml(&root, "docs/release/version-metadata-contract.yaml");
    let matrix: CompatibilityMatrix = read_yaml(&root, "docs/release/compatibility-matrix.yaml");
    let manifest: toml::Value = fs::read_to_string(root.join("Cargo.toml"))
        .expect("Cargo.toml should be readable")
        .parse()
        .expect("Cargo.toml should parse");

    assert_eq!(matrix.matrix_version, "compatibility-matrix-v1");
    assert_eq!(matrix.current_engine_version, mabi_core::RELEASE_VERSION);
    assert_eq!(
        matrix.workspace_rust_version,
        manifest["workspace"]["package"]["rust-version"]
            .as_str()
            .expect("workspace rust-version should be a string")
    );
    assert_eq!(
        matrix.version_metadata_contract,
        "version-metadata-contract-v1"
    );
    assert_eq!(
        matrix.readiness_matrix_version,
        "protocol-readiness-matrix-v1"
    );
    assert_eq!(matrix.release_channel, "source-build");

    let supported = string_set(&matrix.supported_contract_versions);
    for required in &contract.required_contract_versions {
        assert!(
            supported.contains(required.as_str()),
            "compatibility matrix should support {required}"
        );
    }

    for path in [
        &matrix.runner_policy_document,
        &matrix.release_policy_document,
        &matrix.changelog_policy_document,
    ] {
        assert!(
            root.join(path).is_file(),
            "release policy path should exist: {path}"
        );
    }
    assert_eq!(
        matrix.compatibility_decision_owner,
        "mabinogion-trials-or-forge"
    );
    assert!(matrix
        .ownership_boundary
        .mabinogion_owns
        .iter()
        .any(|entry| entry == "engine_release_metadata"));
    assert!(matrix
        .ownership_boundary
        .not_owned_by_mabinogion
        .iter()
        .any(|entry| entry == "trial_suite_allow_deny_decision"));
}

#[test]
fn version_output_matches_release_compatibility_matrix() {
    let root = repo_root();
    let matrix: CompatibilityMatrix = read_yaml(&root, "docs/release/compatibility-matrix.yaml");
    let envelope = mabi_version_json();
    let data = &envelope["data"];

    assert_eq!(
        data["engine_version"],
        matrix.current_engine_version.as_str()
    );
    assert_eq!(
        data["release_metadata"]["engine_version"],
        matrix.current_engine_version.as_str()
    );
    assert_eq!(
        data["release_metadata"]["workspace_rust_version"],
        matrix.workspace_rust_version.as_str()
    );
    assert_eq!(
        data["release_metadata"]["release_channel"],
        matrix.release_channel.as_str()
    );
    assert_eq!(
        data["release_metadata"]["compatibility_matrix_version"],
        matrix.matrix_version.as_str()
    );
    assert_eq!(
        data["release_metadata"]["compatibility_matrix_document"],
        "docs/release/compatibility-matrix.yaml"
    );
    assert_eq!(
        data["trial_compatible_metadata"]["compatible_trial_suite_range"],
        matrix.compatible_trial_suite_range.as_str()
    );
    assert_eq!(
        data["trial_compatible_metadata"]["compatibility_decision_owner"],
        matrix.compatibility_decision_owner.as_str()
    );
    assert_eq!(
        data["trial_compatible_metadata"]["runner_policy_document"],
        matrix.runner_policy_document.as_str()
    );

    let cli_contract_versions = data["contract_versions"]
        .as_object()
        .expect("contract_versions should be an object")
        .values()
        .map(|value| value.as_str().expect("contract version should be a string"))
        .collect::<HashSet<_>>();
    for supported in &matrix.supported_contract_versions {
        assert!(
            cli_contract_versions.contains(supported.as_str()),
            "CLI version output should report {supported}"
        );
    }
}

#[test]
fn protocol_capability_metadata_matches_readiness_matrix() {
    let root = repo_root();
    let matrix: CompatibilityMatrix = read_yaml(&root, "docs/release/compatibility-matrix.yaml");
    let readiness: ReadinessMatrix = read_yaml(
        &root,
        "docs/protocol-readiness/protocol-readiness-matrix.yaml",
    );
    let envelope = mabi_version_json();

    assert_eq!(readiness.matrix_version, matrix.readiness_matrix_version);
    let readiness_profiles = readiness
        .protocols
        .iter()
        .map(|protocol| {
            (
                protocol.id.as_str(),
                protocol
                    .profiles
                    .iter()
                    .map(|profile| profile.trial_profile_id.as_str())
                    .collect::<HashSet<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();

    let cli_protocols = envelope["data"]["registered_protocols"]
        .as_array()
        .expect("registered protocols should be an array")
        .iter()
        .map(|protocol| {
            (
                protocol["protocol_key"]
                    .as_str()
                    .expect("protocol key should be a string"),
                protocol,
            )
        })
        .collect::<HashMap<_, _>>();

    assert_eq!(matrix.protocol_capabilities.len(), 4);
    for capability in &matrix.protocol_capabilities {
        let readiness_ids = readiness_profiles
            .get(capability.protocol_key.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "readiness matrix should include {}",
                    capability.protocol_key
                )
            });
        let capability_profiles = string_set(&capability.trial_profile_ids);
        assert_eq!(
            capability_profiles,
            readiness_ids.clone(),
            "{} capability metadata should mirror readiness profile ids",
            capability.protocol_key
        );
        assert_eq!(
            capability.readiness_contract_version,
            "unified-readiness-contract-v1"
        );
        assert!(
            capability
                .breaking_change_scope
                .iter()
                .any(|scope| scope == "version_metadata_contract"),
            "{} should classify version metadata changes",
            capability.protocol_key
        );

        let cli = cli_protocols
            .get(capability.protocol_key.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "CLI version output should include {}",
                    capability.protocol_key
                )
            });
        assert_eq!(
            cli["capability_version"],
            capability.capability_version.as_str()
        );
        assert_eq!(
            cli["readiness_matrix_version"],
            capability.readiness_matrix_version.as_str()
        );
        assert_eq!(
            cli["readiness_contract_version"],
            capability.readiness_contract_version.as_str()
        );
        let cli_profiles = cli["trial_profile_ids"]
            .as_array()
            .expect("trial_profile_ids should be an array")
            .iter()
            .map(|value| value.as_str().expect("trial_profile_id should be a string"))
            .collect::<HashSet<_>>();
        assert_eq!(
            cli_profiles,
            string_set(&capability.trial_profile_ids),
            "CLI protocol metadata should match release matrix for {}",
            capability.protocol_key
        );
    }
}

#[test]
fn forge_and_trials_remain_compatibility_decision_owners() {
    let root = repo_root();
    let matrix: CompatibilityMatrix = read_yaml(&root, "docs/release/compatibility-matrix.yaml");
    let envelope = mabi_version_json();

    assert_eq!(
        matrix.compatibility_decision_owner,
        "mabinogion-trials-or-forge"
    );
    assert_eq!(
        envelope["data"]["trial_compatible_metadata"]["compatibility_decision_owner"],
        "mabinogion-trials-or-forge"
    );
    assert!(matrix
        .ownership_boundary
        .not_owned_by_mabinogion
        .iter()
        .any(|entry| entry == "trial_suite_allow_deny_decision"));
    assert!(matrix
        .ownership_boundary
        .not_owned_by_mabinogion
        .iter()
        .any(|entry| entry == "certification_issuance"));
}
