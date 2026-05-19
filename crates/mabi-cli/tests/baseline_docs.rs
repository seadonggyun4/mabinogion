use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct Baseline {
    schema_version: u16,
    phase: String,
    workspace: Workspace,
    runtime_contract: RuntimeContract,
    evidence_contract: EvidenceContract,
    release_version_contract: ReleaseVersionContract,
    cli_surface: CliSurface,
    protocol_verification: ProtocolVerification,
    ownership_boundaries: OwnershipBoundaries,
}

#[derive(Debug, Deserialize)]
struct Workspace {
    manifest: String,
    crates: Vec<CrateEntry>,
}

#[derive(Debug, Deserialize)]
struct CrateEntry {
    id: String,
    path: String,
    role: String,
    public_surface: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeContract {
    contract_status: String,
    source_refs: Vec<SourceRef>,
    lifecycle_flow: Vec<String>,
    next_phase_gaps: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SourceRef {
    path: String,
    symbols: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EvidenceContract {
    contract_status: String,
    contract_docs: Vec<String>,
    source_refs: Vec<SourceRef>,
    ownership_boundary: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ReleaseVersionContract {
    contract_status: String,
    contract_docs: Vec<String>,
    source_refs: Vec<SourceRef>,
    compatibility_matrix: ReleaseCompatibilityMatrix,
    ownership_boundary: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ReleaseCompatibilityMatrix {
    matrix_version: String,
    current_engine_version_source: String,
    decision_owner: String,
}

#[derive(Debug, Deserialize)]
struct CliSurface {
    contract_status: String,
    output_contract_status: String,
    contract_docs: Vec<String>,
    source_ref: String,
    registry_ref: String,
    commands: Vec<CommandEntry>,
    next_phase_gaps: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CommandEntry {
    id: String,
    source_variant: String,
    status: String,
    surface: String,
}

#[derive(Debug, Deserialize)]
struct ProtocolVerification {
    unified_readiness_contract_status: String,
    contract_docs: Vec<String>,
    expected_unified_fields: Vec<String>,
    protocols: Vec<ProtocolEntry>,
}

#[derive(Debug, Deserialize)]
struct ProtocolEntry {
    id: String,
    #[serde(rename = "crate")]
    crate_id: String,
    status: String,
    verification_contract: Option<String>,
    verification_baseline: Option<String>,
    readiness_contract_gap: bool,
    related_docs: Option<Vec<String>>,
    notes: String,
}

#[derive(Debug, Deserialize)]
struct OwnershipBoundaries {
    mabinogion_owns: Vec<String>,
    not_owned_by_mabinogion: Vec<String>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root should resolve")
}

fn load_baseline(root: &Path) -> Baseline {
    let baseline_path = root.join("docs/baseline/current-baseline.yaml");
    let contents = fs::read_to_string(&baseline_path).unwrap_or_else(|error| {
        panic!(
            "failed to read baseline at {}: {error}",
            baseline_path.display()
        )
    });
    serde_yaml::from_str(&contents).expect("baseline YAML should parse")
}

#[test]
fn baseline_crates_match_workspace_members() {
    let root = repo_root();
    let baseline = load_baseline(&root);

    assert_eq!(baseline.schema_version, 1);
    assert_eq!(baseline.phase, "phase_0_current_baseline_reconciliation");

    let manifest_path = root.join(&baseline.workspace.manifest);
    let manifest = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", manifest_path.display()));
    let manifest: toml::Value = manifest.parse().expect("workspace Cargo.toml should parse");
    let members = manifest["workspace"]["members"]
        .as_array()
        .expect("workspace members should be an array")
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("workspace member should be a string")
                .to_owned()
        })
        .collect::<HashSet<_>>();

    let documented = baseline
        .workspace
        .crates
        .iter()
        .map(|entry| {
            assert!(!entry.id.trim().is_empty(), "crate id must not be empty");
            assert!(
                !entry.role.trim().is_empty(),
                "{} role must not be empty",
                entry.id
            );
            assert!(
                !entry.public_surface.trim().is_empty(),
                "{} public surface must not be empty",
                entry.id
            );
            assert!(
                root.join(&entry.path).is_dir(),
                "{} path should exist",
                entry.path
            );
            entry.path.clone()
        })
        .collect::<HashSet<_>>();

    assert_eq!(
        documented, members,
        "baseline workspace crate paths should match Cargo.toml members"
    );
}

#[test]
fn runtime_contract_refs_existing_symbols() {
    let root = repo_root();
    let baseline = load_baseline(&root);

    assert_eq!(
        baseline.runtime_contract.contract_status,
        "frozen_contract_v1"
    );
    for required_step in [
        "construct_runtime_session",
        "start_managed_services",
        "wait_for_readiness",
        "capture_service_snapshots",
        "stop_managed_services",
    ] {
        assert!(
            baseline
                .runtime_contract
                .lifecycle_flow
                .iter()
                .any(|step| step == required_step),
            "baseline lifecycle should include {required_step}"
        );
    }
    assert!(
        baseline
            .runtime_contract
            .next_phase_gaps
            .iter()
            .any(|gap| gap.contains("trial run")),
        "runtime baseline should point remaining execution work at future trial run"
    );

    for source_ref in &baseline.runtime_contract.source_refs {
        let path = root.join(&source_ref.path);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for symbol in &source_ref.symbols {
            assert!(
                contents.contains(symbol),
                "{} should contain documented symbol {}",
                source_ref.path,
                symbol
            );
        }
    }
}

#[test]
fn evidence_contract_refs_existing_docs_and_symbols() {
    let root = repo_root();
    let baseline = load_baseline(&root);

    assert_eq!(
        baseline.evidence_contract.contract_status,
        "run_evidence_schema_v1"
    );
    for contract_doc in &baseline.evidence_contract.contract_docs {
        assert!(
            root.join(contract_doc).is_file(),
            "Run Evidence contract doc should exist: {contract_doc}"
        );
    }
    assert!(
        baseline
            .evidence_contract
            .ownership_boundary
            .iter()
            .any(|entry| entry.contains("execution evidence export")),
        "Run Evidence boundary should keep mabinogion focused on evidence export"
    );
    assert!(
        baseline
            .evidence_contract
            .ownership_boundary
            .iter()
            .any(|entry| entry.contains("scoring")),
        "Run Evidence boundary should exclude scoring"
    );

    for source_ref in &baseline.evidence_contract.source_refs {
        let path = root.join(&source_ref.path);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for symbol in &source_ref.symbols {
            assert!(
                contents.contains(symbol),
                "{} should contain documented evidence symbol {}",
                source_ref.path,
                symbol
            );
        }
    }
}

#[test]
fn release_version_contract_refs_existing_docs_and_symbols() {
    let root = repo_root();
    let baseline = load_baseline(&root);

    assert_eq!(
        baseline.release_version_contract.contract_status,
        "version_metadata_contract_v1"
    );
    for contract_doc in &baseline.release_version_contract.contract_docs {
        assert!(
            root.join(contract_doc).is_file(),
            "release/version contract doc should exist: {contract_doc}"
        );
    }
    assert_eq!(
        baseline
            .release_version_contract
            .compatibility_matrix
            .matrix_version,
        "compatibility-matrix-v1"
    );
    assert!(
        baseline
            .release_version_contract
            .compatibility_matrix
            .current_engine_version_source
            .contains("Cargo.toml"),
        "release version baseline should point at Cargo.toml as the engine version source"
    );
    assert_eq!(
        baseline
            .release_version_contract
            .compatibility_matrix
            .decision_owner,
        "mabinogion-trials-or-forge"
    );
    assert!(
        baseline
            .release_version_contract
            .ownership_boundary
            .iter()
            .any(|entry| entry.contains("metadata export")),
        "release baseline should keep mabinogion focused on metadata export"
    );
    assert!(
        baseline
            .release_version_contract
            .ownership_boundary
            .iter()
            .any(|entry| entry.contains("allow deny")),
        "release baseline should exclude compatibility decisions"
    );

    for source_ref in &baseline.release_version_contract.source_refs {
        let path = root.join(&source_ref.path);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for symbol in &source_ref.symbols {
            assert!(
                contents.contains(symbol),
                "{} should contain documented release/version symbol {}",
                source_ref.path,
                symbol
            );
        }
    }
}

#[test]
fn cli_surface_refs_current_command_variants() {
    let root = repo_root();
    let baseline = load_baseline(&root);

    assert_eq!(baseline.cli_surface.contract_status, "implemented_baseline");
    assert_eq!(
        baseline.cli_surface.output_contract_status,
        "local_runner_contract_v1"
    );
    assert!(
        baseline
            .cli_surface
            .next_phase_gaps
            .iter()
            .any(|gap| gap.contains("trial run implementation")),
        "CLI baseline should record future trial run implementation as a gap"
    );
    for contract_doc in &baseline.cli_surface.contract_docs {
        assert!(
            root.join(contract_doc).is_file(),
            "Local Runner contract doc should exist: {contract_doc}"
        );
    }

    let main_path = root.join(&baseline.cli_surface.source_ref);
    let main_source = fs::read_to_string(&main_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", main_path.display()));
    let registry_path = root.join(&baseline.cli_surface.registry_ref);
    let registry_source = fs::read_to_string(&registry_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", registry_path.display()));

    for protocol in ["mabi_modbus", "mabi_opcua", "mabi_bacnet", "mabi_knx"] {
        assert!(
            registry_source.contains(protocol),
            "runtime registry should still register {protocol}"
        );
    }

    let expected = [
        "serve", "scenario", "chaos", "inspect", "validate", "control", "generate", "doctor",
        "version",
    ]
    .into_iter()
    .collect::<HashSet<_>>();
    let documented = baseline
        .cli_surface
        .commands
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<HashSet<_>>();
    assert_eq!(documented, expected);

    for command in &baseline.cli_surface.commands {
        assert!(
            command.status.starts_with("implemented"),
            "{} should remain implemented in the PHASE 0 baseline",
            command.id
        );
        assert!(
            !command.surface.trim().is_empty(),
            "{} surface must not be empty",
            command.id
        );
        let tuple_variant = format!("{}(", command.source_variant);
        let unit_variant = format!("    {},", command.source_variant);
        assert!(
            main_source.contains(&tuple_variant) || main_source.contains(&unit_variant),
            "{} should exist as Commands::{} in {}",
            command.id,
            command.source_variant,
            baseline.cli_surface.source_ref
        );
    }
}

#[test]
fn protocol_verification_baseline_tracks_unified_readiness_contracts() {
    let root = repo_root();
    let baseline = load_baseline(&root);

    assert_eq!(
        baseline
            .protocol_verification
            .unified_readiness_contract_status,
        "unified_readiness_contract_v1"
    );
    let required_unified_fields = [
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
    ];
    for field in required_unified_fields {
        assert!(
            baseline
                .protocol_verification
                .expected_unified_fields
                .iter()
                .any(|documented| documented == field),
            "Unified Readiness field {field} should be documented"
        );
    }
    for contract_doc in &baseline.protocol_verification.contract_docs {
        assert!(
            root.join(contract_doc).is_file(),
            "Unified Readiness contract doc should exist: {contract_doc}"
        );
    }

    let protocols = baseline
        .protocol_verification
        .protocols
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<HashMap<_, _>>();

    for id in ["bacnet", "knx", "modbus", "opcua"] {
        let protocol = protocols
            .get(id)
            .unwrap_or_else(|| panic!("{id} should be documented"));
        assert_eq!(protocol.status, "contract_present");
        assert!(!protocol.readiness_contract_gap);
        assert!(
            root.join(protocol.verification_contract.as_ref().unwrap())
                .is_file(),
            "{id} verification contract should exist"
        );
        assert!(
            root.join(protocol.verification_baseline.as_ref().unwrap())
                .is_file(),
            "{id} verification baseline should exist"
        );
        assert!(
            !protocol.notes.trim().is_empty(),
            "{id} should include baseline notes"
        );
        if let Some(related_docs) = &protocol.related_docs {
            for related_doc in related_docs {
                assert!(
                    root.join(related_doc).is_file(),
                    "{id} related doc should exist: {related_doc}"
                );
            }
        }
    }

    for protocol in baseline.protocol_verification.protocols {
        assert!(
            root.join("crates").join(&protocol.crate_id).is_dir(),
            "{} crate path should exist",
            protocol.crate_id
        );
    }
}

#[test]
fn ownership_boundary_keeps_scoring_and_certification_out_of_mabinogion() {
    let root = repo_root();
    let baseline = load_baseline(&root);

    for owned in [
        "protocol_session_execution",
        "shared_runtime_contract",
        "installed_cli_surface",
        "run_evidence_export_primitives",
        "run_evidence_schema",
        "release_version_metadata",
    ] {
        assert!(
            baseline
                .ownership_boundaries
                .mabinogion_owns
                .iter()
                .any(|entry| entry == owned),
            "mabinogion should own {owned}"
        );
    }

    for excluded in [
        "trial_definition_authoring",
        "scoring_policy",
        "certification_issuance",
    ] {
        assert!(
            baseline
                .ownership_boundaries
                .not_owned_by_mabinogion
                .iter()
                .any(|entry| entry == excluded),
            "mabinogion should not own {excluded}"
        );
    }
}
