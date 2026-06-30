use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct UnifiedContract {
    schema_version: u16,
    contract_id: String,
    profile_required_fields: Vec<String>,
    enums: ContractEnums,
    protocols: Vec<UnifiedProtocolRef>,
}

#[derive(Debug, Deserialize)]
struct ContractEnums {
    lanes: Vec<String>,
    coverage_status: Vec<String>,
    optional_interop_policy: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct UnifiedProtocolRef {
    id: String,
    #[serde(rename = "crate")]
    crate_id: String,
    contract: String,
    baseline: String,
}

#[derive(Debug, Deserialize)]
struct ReadinessMatrix {
    schema_version: u16,
    contract_id: String,
    protocols: Vec<MatrixProtocol>,
}

#[derive(Debug, Deserialize)]
struct MatrixProtocol {
    id: String,
    profiles: Vec<MatrixProfile>,
}

#[derive(Debug, Deserialize)]
struct MatrixProfile {
    capability_id: String,
    profile_id: String,
    trial_profile_id: String,
    trial_level: u8,
    lane: String,
    coverage_status: String,
    optional_interop_policy: String,
}

#[derive(Debug, Deserialize)]
struct ProtocolContract {
    version: u16,
    baseline: ProtocolBaseline,
    capabilities: Vec<Capability>,
    profiles: Vec<ProtocolProfile>,
    unified_readiness: ProtocolReadiness,
}

#[derive(Debug, Deserialize)]
struct ProtocolBaseline {
    #[serde(rename = "crate")]
    crate_id: String,
}

#[derive(Debug, Deserialize)]
struct Capability {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ProtocolProfile {
    id: String,
    capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ProtocolReadiness {
    contract_id: String,
    protocol: String,
    baseline: String,
    matrix_ref: String,
    profiles: Vec<ReadinessProfile>,
}

#[derive(Debug, Deserialize)]
struct ReadinessProfile {
    capability_id: String,
    profile_id: String,
    lane: String,
    coverage_status: String,
    optional_interop_policy: String,
    trial_profile_id: String,
    trial_level: u8,
    required_evidence: Vec<String>,
    engine_requirement: String,
    forge_display_label: String,
    source_refs: Vec<String>,
    test_refs: Vec<String>,
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

fn assert_existing_refs(root: &Path, refs: &[String], context: &str) {
    for reference in refs {
        assert!(
            root.join(reference).exists(),
            "{context} should reference an existing path: {reference}"
        );
    }
}

fn assert_trial_profile_id(protocol: &str, profile: &ReadinessProfile) {
    let parts = profile.trial_profile_id.split('.').collect::<Vec<_>>();
    assert_eq!(
        parts.len(),
        3,
        "{} trial_profile_id should use protocol.lN.capability format",
        profile.profile_id
    );
    assert_eq!(parts[0], protocol);
    assert_eq!(parts[1], format!("l{}", profile.trial_level));
    assert!(
        (1..=5).contains(&profile.trial_level),
        "{} trial_level should be in the public MVP level range",
        profile.profile_id
    );
    assert!(
        !parts[2].trim().is_empty(),
        "{} trial_profile_id capability suffix must not be empty",
        profile.profile_id
    );
}

#[test]
fn unified_readiness_contract_declares_required_fields_and_protocols() {
    let root = repo_root();
    let contract: UnifiedContract = read_yaml(
        &root,
        "docs/protocol-readiness/unified-readiness-contract.yaml",
    );

    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.contract_id, "unified-readiness-contract-v1");

    for field in [
        "capability_id",
        "profile_id",
        "lane",
        "coverage_status",
        "optional_interop_policy",
        "trial_profile_id",
        "trial_level",
        "required_evidence",
        "engine_requirement",
        "forge_display_label",
    ] {
        assert!(
            contract
                .profile_required_fields
                .iter()
                .any(|documented| documented == field),
            "Unified Readiness Contract should require {field}"
        );
    }

    assert_eq!(
        contract
            .enums
            .lanes
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>(),
        [
            "deterministic",
            "ignored_interop",
            "release_only_perf",
            "artifact_only_capture"
        ]
        .into_iter()
        .collect::<HashSet<_>>()
    );
    assert_eq!(
        contract
            .enums
            .coverage_status
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>(),
        [
            "implemented",
            "partial",
            "planned",
            "unsupported_recorded",
            "future"
        ]
        .into_iter()
        .collect::<HashSet<_>>()
    );
    assert_eq!(
        contract
            .enums
            .optional_interop_policy
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>(),
        [
            "none",
            "nightly",
            "manual",
            "release_only",
            "capture_only",
            "future"
        ]
        .into_iter()
        .collect::<HashSet<_>>()
    );

    let expected_protocols = ["opcua", "bacnet", "modbus", "knx"];
    let protocols = contract
        .protocols
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<HashMap<_, _>>();
    assert_eq!(protocols.len(), expected_protocols.len());

    for id in expected_protocols {
        let protocol = protocols
            .get(id)
            .unwrap_or_else(|| panic!("{id} should be listed in the unified contract"));
        assert!(
            root.join("crates").join(&protocol.crate_id).is_dir(),
            "{id} crate should exist"
        );
        assert!(
            root.join(&protocol.contract).is_file(),
            "{id} protocol contract should exist"
        );
        assert!(
            root.join(&protocol.baseline).is_file(),
            "{id} protocol baseline should exist"
        );
    }
}

#[test]
fn protocol_contracts_follow_unified_readiness_shape() {
    let root = repo_root();
    let contract: UnifiedContract = read_yaml(
        &root,
        "docs/protocol-readiness/unified-readiness-contract.yaml",
    );
    let allowed_lanes = contract
        .enums
        .lanes
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let allowed_statuses = contract
        .enums
        .coverage_status
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let allowed_policies = contract
        .enums
        .optional_interop_policy
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();

    for protocol_ref in &contract.protocols {
        let protocol: ProtocolContract = read_yaml(&root, &protocol_ref.contract);
        assert_eq!(protocol.version, 1);
        assert_eq!(protocol.baseline.crate_id, protocol_ref.crate_id);
        assert_eq!(protocol.unified_readiness.contract_id, contract.contract_id);
        assert_eq!(protocol.unified_readiness.protocol, protocol_ref.id);
        assert_eq!(protocol.unified_readiness.baseline, protocol_ref.baseline);
        assert!(
            root.join(&protocol.unified_readiness.matrix_ref).is_file(),
            "{} matrix ref should exist",
            protocol_ref.id
        );

        let capabilities = protocol
            .capabilities
            .iter()
            .map(|capability| capability.id.as_str())
            .collect::<HashSet<_>>();
        let profiles = protocol
            .profiles
            .iter()
            .map(|profile| {
                for capability in &profile.capabilities {
                    assert!(
                        capabilities.contains(capability.as_str()),
                        "{} profile {} should reference existing capability {}",
                        protocol_ref.id,
                        profile.id,
                        capability
                    );
                }
                profile.id.as_str()
            })
            .collect::<HashSet<_>>();

        let mut readiness_profile_ids = HashSet::new();
        for profile in &protocol.unified_readiness.profiles {
            assert!(
                readiness_profile_ids.insert(profile.profile_id.as_str()),
                "{} profile id should be unique: {}",
                protocol_ref.id,
                profile.profile_id
            );
            assert!(
                capabilities.contains(profile.capability_id.as_str()),
                "{} unified profile {} should reference existing capability {}",
                protocol_ref.id,
                profile.profile_id,
                profile.capability_id
            );
            assert!(
                profiles.contains(profile.profile_id.as_str()),
                "{} unified profile {} should reference existing protocol profile",
                protocol_ref.id,
                profile.profile_id
            );
            assert!(
                allowed_lanes.contains(profile.lane.as_str()),
                "{} profile {} has invalid lane {}",
                protocol_ref.id,
                profile.profile_id,
                profile.lane
            );
            assert!(
                allowed_statuses.contains(profile.coverage_status.as_str()),
                "{} profile {} has invalid coverage status {}",
                protocol_ref.id,
                profile.profile_id,
                profile.coverage_status
            );
            assert!(
                allowed_policies.contains(profile.optional_interop_policy.as_str()),
                "{} profile {} has invalid optional interop policy {}",
                protocol_ref.id,
                profile.profile_id,
                profile.optional_interop_policy
            );
            assert_trial_profile_id(&protocol_ref.id, profile);
            assert!(
                !profile.required_evidence.is_empty(),
                "{} profile {} must require evidence",
                protocol_ref.id,
                profile.profile_id
            );
            assert!(
                !profile.engine_requirement.trim().is_empty(),
                "{} profile {} must describe engine requirements",
                protocol_ref.id,
                profile.profile_id
            );
            assert!(
                !profile.forge_display_label.trim().is_empty(),
                "{} profile {} must have a legacy Forge display label retained until contract v2",
                protocol_ref.id,
                profile.profile_id
            );
            assert_existing_refs(
                &root,
                &profile.source_refs,
                &format!("{} {}", protocol_ref.id, profile.profile_id),
            );
            assert_existing_refs(
                &root,
                &profile.test_refs,
                &format!("{} {}", protocol_ref.id, profile.profile_id),
            );
        }
    }
}

#[test]
fn readiness_matrix_matches_protocol_contract_profiles() {
    let root = repo_root();
    let contract: UnifiedContract = read_yaml(
        &root,
        "docs/protocol-readiness/unified-readiness-contract.yaml",
    );
    let matrix: ReadinessMatrix = read_yaml(
        &root,
        "docs/protocol-readiness/protocol-readiness-matrix.yaml",
    );

    assert_eq!(matrix.schema_version, 1);
    assert_eq!(matrix.contract_id, contract.contract_id);

    let matrix_by_protocol = matrix
        .protocols
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<HashMap<_, _>>();

    for protocol_ref in &contract.protocols {
        let protocol: ProtocolContract = read_yaml(&root, &protocol_ref.contract);
        let matrix_protocol = matrix_by_protocol
            .get(protocol_ref.id.as_str())
            .unwrap_or_else(|| panic!("matrix should include {}", protocol_ref.id));

        let unified_profiles = protocol
            .unified_readiness
            .profiles
            .iter()
            .map(|profile| (profile.profile_id.as_str(), profile))
            .collect::<HashMap<_, _>>();
        assert_eq!(
            matrix_protocol.profiles.len(),
            unified_profiles.len(),
            "{} matrix profile count should match protocol contract",
            protocol_ref.id
        );

        for matrix_profile in &matrix_protocol.profiles {
            let protocol_profile = unified_profiles
                .get(matrix_profile.profile_id.as_str())
                .unwrap_or_else(|| {
                    panic!(
                        "{} matrix profile {} should exist in protocol contract",
                        protocol_ref.id, matrix_profile.profile_id
                    )
                });
            assert_eq!(matrix_profile.capability_id, protocol_profile.capability_id);
            assert_eq!(
                matrix_profile.trial_profile_id,
                protocol_profile.trial_profile_id
            );
            assert_eq!(matrix_profile.trial_level, protocol_profile.trial_level);
            assert_eq!(matrix_profile.lane, protocol_profile.lane);
            assert_eq!(
                matrix_profile.coverage_status,
                protocol_profile.coverage_status
            );
            assert_eq!(
                matrix_profile.optional_interop_policy,
                protocol_profile.optional_interop_policy
            );
        }
    }
}
