use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct VerificationContract {
    baseline: ContractBaseline,
    policies: ContractPolicies,
    capabilities: Vec<ContractEntry>,
    profiles: Vec<ContractEntry>,
    peers: Vec<ContractEntry>,
}

#[derive(Debug, Deserialize)]
struct ContractBaseline {
    capture_corpus_present: bool,
}

#[derive(Debug, Deserialize)]
struct ContractPolicies {
    capture_lane: String,
    gui_automation: String,
    production_dependency_policy: String,
}

#[derive(Debug, Deserialize)]
struct ContractEntry {
    id: String,
}

#[derive(Debug, Deserialize)]
struct CaptureCatalog {
    version: u32,
    captures: Vec<CaptureCatalogEntry>,
}

#[derive(Debug, Deserialize)]
struct CaptureCatalogEntry {
    id: String,
    source: String,
    source_tool: String,
    license_note: String,
    protocol_area: String,
    profile_ids: Vec<String>,
    capability_ids: Vec<String>,
    artifact_dir: String,
    expected_behavior: String,
    ci_executable: bool,
    curation_state: String,
    artifacts: Vec<CaptureArtifact>,
}

#[derive(Debug, Deserialize)]
struct CaptureArtifact {
    role: String,
    path: String,
    format: String,
}

#[derive(Debug, Deserialize)]
struct CaptureManifest {
    version: u32,
    id: String,
    source: String,
    source_tool: String,
    license_note: String,
    protocol_area: String,
    profile_ids: Vec<String>,
    capability_ids: Vec<String>,
    expected_behavior: String,
    ci_executable: bool,
    curation_state: String,
    notes: String,
}

#[derive(Debug, Deserialize)]
struct CaptureReplay {
    version: u32,
    peer: String,
    protocol_area: String,
    profile_ids: Vec<String>,
    capability_ids: Vec<String>,
    steps: Vec<CaptureStep>,
    expected_outcomes: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct CaptureStep {
    id: String,
    kind: String,
    request: Option<String>,
    group_address: Option<String>,
    value: Option<Value>,
    expect: String,
}

#[derive(Debug, Deserialize)]
struct PeerTranscript {
    schema_version: u32,
    target: String,
    peer: String,
    sut_addr: String,
    capabilities: Vec<TranscriptResult>,
    steps: Vec<TranscriptResult>,
    failure_category: Option<String>,
    errors: Vec<String>,
    artifacts: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct TranscriptResult {
    id: String,
    status: String,
    detail: String,
}

#[derive(Debug, Deserialize)]
struct PacketSummary {
    version: u32,
    tool: String,
    packets: Vec<PacketSummaryItem>,
}

#[derive(Debug, Deserialize)]
struct PacketSummaryItem {
    direction: String,
    service: String,
    purpose: String,
}

#[derive(Debug, Deserialize)]
struct TraceSummary {
    version: u32,
    tool: String,
    events: Vec<TraceSummaryItem>,
}

#[derive(Debug, Deserialize)]
struct TraceSummaryItem {
    id: String,
    classification: String,
}

#[derive(Debug, Deserialize)]
struct ModelReference {
    version: u32,
    tool: String,
    model_kind: String,
    group_objects: Vec<ModelGroupObject>,
    secure: ModelSecureMarker,
}

#[derive(Debug, Deserialize)]
struct ModelGroupObject {
    name: String,
    address: String,
    dpt: String,
    capability: String,
}

#[derive(Debug, Deserialize)]
struct ModelSecureMarker {
    tracked: bool,
    implemented: bool,
    capability: String,
}

const CONTRACT: &str = include_str!("../../../docs/knx-simulator/verification-contract.yaml");

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("mabi-knx crate should live two levels below repo root")
        .to_path_buf()
}

fn captures_root() -> PathBuf {
    repo_root().join("verification/knx/captures")
}

fn capture_catalog_path() -> PathBuf {
    captures_root().join("catalog.toml")
}

fn load_contract() -> VerificationContract {
    serde_yaml::from_str(CONTRACT).expect("KNX verification contract should parse")
}

fn load_capture_catalog() -> CaptureCatalog {
    toml::from_str(
        &fs::read_to_string(capture_catalog_path()).expect("capture catalog should exist"),
    )
    .expect("capture catalog should parse")
}

fn resolve_capture_path(relative: &str) -> PathBuf {
    captures_root().join(relative)
}

fn ids(entries: &[ContractEntry]) -> HashSet<&str> {
    entries.iter().map(|entry| entry.id.as_str()).collect()
}

#[test]
fn capture_catalog_matches_phase_4_policy_contract() {
    let contract = load_contract();
    assert!(contract.baseline.capture_corpus_present);
    assert_eq!(contract.policies.capture_lane, "artifact_only");
    assert_eq!(contract.policies.gui_automation, "out_of_scope");
    assert_eq!(
        contract.policies.production_dependency_policy,
        "verification_assets_only"
    );

    let catalog = load_capture_catalog();
    assert_eq!(catalog.version, 1);
    assert_eq!(
        catalog.captures.len(),
        4,
        "Phase 4 should seed XKNX, Calimero, knxd, and OpenKNX corpus entries"
    );

    let peer_ids = ids(&contract.peers);
    let profile_ids = ids(&contract.profiles);
    let capability_ids = ids(&contract.capabilities);

    let mut tools = HashSet::new();
    for capture in &catalog.captures {
        assert!(!capture.id.trim().is_empty());
        assert!(!capture.source.trim().is_empty());
        assert!(!capture.license_note.trim().is_empty());
        assert!(!capture.protocol_area.trim().is_empty());
        assert!(!capture.expected_behavior.trim().is_empty());
        assert!(!capture.curation_state.trim().is_empty());
        assert!(
            !capture.ci_executable,
            "capture {} must stay artifact-only and non-executable",
            capture.id
        );
        assert!(
            peer_ids.contains(capture.source_tool.as_str()),
            "capture {} references unknown source tool {}",
            capture.id,
            capture.source_tool
        );
        assert!(!capture.profile_ids.is_empty());
        assert!(!capture.capability_ids.is_empty());
        assert!(!capture.artifacts.is_empty());
        tools.insert(capture.source_tool.as_str());

        for profile_id in &capture.profile_ids {
            assert!(
                profile_ids.contains(profile_id.as_str()),
                "capture {} references unknown profile {}",
                capture.id,
                profile_id
            );
        }
        for capability_id in &capture.capability_ids {
            assert!(
                capability_ids.contains(capability_id.as_str()),
                "capture {} references unknown capability {}",
                capture.id,
                capability_id
            );
        }
    }

    for expected_tool in ["xknx", "calimero_tools", "knxd", "openknx_thelsing"] {
        assert!(
            tools.contains(expected_tool),
            "capture catalog missing {expected_tool} seed"
        );
    }
}

#[test]
fn capture_artifacts_are_rooted_and_replayable() {
    let root = captures_root()
        .canonicalize()
        .expect("captures root should exist on disk");
    let catalog = load_capture_catalog();

    for capture in &catalog.captures {
        let artifact_dir = resolve_capture_path(&capture.artifact_dir)
            .canonicalize()
            .unwrap_or_else(|_| panic!("artifact dir missing for {}", capture.id));
        assert!(
            artifact_dir.starts_with(&root),
            "artifact dir for {} must stay inside verification/knx/captures",
            capture.id
        );

        let mut saw_manifest = false;
        let mut saw_replay = false;
        let mut saw_runbook = false;

        for artifact in &capture.artifacts {
            let artifact_path = resolve_capture_path(&artifact.path)
                .canonicalize()
                .unwrap_or_else(|_| {
                    panic!("artifact {} missing for {}", artifact.path, capture.id)
                });
            assert!(
                artifact_path.starts_with(&root),
                "artifact {} for {} escapes capture root",
                artifact.path,
                capture.id
            );

            match artifact.role.as_str() {
                "manifest" => {
                    saw_manifest = true;
                    assert_eq!(artifact.format, "toml");
                    let manifest: CaptureManifest = toml::from_str(
                        &fs::read_to_string(&artifact_path).expect("manifest should read"),
                    )
                    .expect("manifest should parse");
                    assert_eq!(manifest.version, 1);
                    assert_eq!(manifest.id, capture.id);
                    assert_eq!(manifest.source, capture.source);
                    assert_eq!(manifest.source_tool, capture.source_tool);
                    assert_eq!(manifest.license_note, capture.license_note);
                    assert_eq!(manifest.protocol_area, capture.protocol_area);
                    assert_eq!(manifest.profile_ids, capture.profile_ids);
                    assert_eq!(manifest.capability_ids, capture.capability_ids);
                    assert_eq!(manifest.expected_behavior, capture.expected_behavior);
                    assert_eq!(manifest.ci_executable, capture.ci_executable);
                    assert_eq!(manifest.curation_state, capture.curation_state);
                    assert!(!manifest.notes.trim().is_empty());
                }
                "replay" => {
                    saw_replay = true;
                    assert_eq!(artifact.format, "json");
                    let replay: CaptureReplay = serde_json::from_str(
                        &fs::read_to_string(&artifact_path).expect("replay should read"),
                    )
                    .expect("replay json should parse");
                    assert_eq!(replay.version, 1);
                    assert_eq!(replay.peer, capture.source_tool);
                    assert_eq!(replay.protocol_area, capture.protocol_area);
                    assert_eq!(replay.profile_ids, capture.profile_ids);
                    assert_eq!(replay.capability_ids, capture.capability_ids);
                    assert!(
                        !replay.steps.is_empty(),
                        "replay for {} must contain at least one step",
                        capture.id
                    );
                    assert!(!replay.expected_outcomes.is_empty());
                    for step in &replay.steps {
                        assert!(!step.id.trim().is_empty());
                        assert!(!step.kind.trim().is_empty());
                        assert!(!step.expect.trim().is_empty());
                        assert!(
                            step.request.is_some()
                                || step.group_address.is_some()
                                || step.value.is_some(),
                            "replay step {} for {} needs replay detail",
                            step.id,
                            capture.id
                        );
                    }
                }
                "transcript" => {
                    assert_eq!(artifact.format, "json");
                    let transcript: PeerTranscript = serde_json::from_str(
                        &fs::read_to_string(&artifact_path).expect("transcript should read"),
                    )
                    .expect("transcript json should parse");
                    assert_eq!(transcript.schema_version, 1);
                    assert_eq!(transcript.peer, capture.source_tool);
                    assert!(!transcript.target.trim().is_empty());
                    assert!(!transcript.sut_addr.trim().is_empty());
                    assert!(transcript.failure_category.is_none());
                    assert!(transcript.errors.is_empty());
                    assert!(!transcript.capabilities.is_empty());
                    assert!(!transcript.steps.is_empty());
                    for result in transcript
                        .capabilities
                        .iter()
                        .chain(transcript.steps.iter())
                    {
                        assert!(!result.id.trim().is_empty());
                        assert_eq!(result.status, "passed");
                        assert!(!result.detail.trim().is_empty());
                    }
                    assert_eq!(
                        transcript.artifacts.get("round_trip_value"),
                        Some(&Value::from(42))
                    );
                }
                "packet_summary" => {
                    assert_eq!(artifact.format, "json");
                    let summary: PacketSummary = serde_json::from_str(
                        &fs::read_to_string(&artifact_path).expect("packet summary should read"),
                    )
                    .expect("packet summary json should parse");
                    assert_eq!(summary.version, 1);
                    assert_eq!(summary.tool, capture.source_tool);
                    assert!(!summary.packets.is_empty());
                    for packet in summary.packets {
                        assert!(!packet.direction.trim().is_empty());
                        assert!(!packet.service.trim().is_empty());
                        assert!(!packet.purpose.trim().is_empty());
                    }
                }
                "trace_summary" => {
                    assert_eq!(artifact.format, "json");
                    let summary: TraceSummary = serde_json::from_str(
                        &fs::read_to_string(&artifact_path).expect("trace summary should read"),
                    )
                    .expect("trace summary json should parse");
                    assert_eq!(summary.version, 1);
                    assert_eq!(summary.tool, capture.source_tool);
                    assert!(!summary.events.is_empty());
                    for event in summary.events {
                        assert!(!event.id.trim().is_empty());
                        assert!(!event.classification.trim().is_empty());
                    }
                }
                "model_reference" => {
                    assert_eq!(artifact.format, "json");
                    let reference: ModelReference = serde_json::from_str(
                        &fs::read_to_string(&artifact_path).expect("model reference should read"),
                    )
                    .expect("model reference json should parse");
                    assert_eq!(reference.version, 1);
                    assert_eq!(reference.tool, capture.source_tool);
                    assert_eq!(reference.model_kind, "metadata_only");
                    assert!(!reference.group_objects.is_empty());
                    for object in reference.group_objects {
                        assert!(!object.name.trim().is_empty());
                        assert!(!object.address.trim().is_empty());
                        assert!(!object.dpt.trim().is_empty());
                        assert!(!object.capability.trim().is_empty());
                    }
                    assert!(reference.secure.tracked);
                    assert!(!reference.secure.implemented);
                    assert_eq!(reference.secure.capability, "secure_future");
                }
                "runbook" => {
                    saw_runbook = true;
                    assert_eq!(artifact.format, "markdown");
                    let content = fs::read_to_string(&artifact_path).expect("runbook should read");
                    assert!(!content.trim().is_empty());
                }
                other => panic!("unsupported capture artifact role {other}"),
            }
        }

        assert!(saw_manifest, "capture {} missing manifest", capture.id);
        assert!(saw_replay, "capture {} missing replay", capture.id);
        assert!(saw_runbook, "capture {} missing runbook", capture.id);
    }
}

#[test]
fn capture_lane_keeps_external_tools_out_of_default_execution() {
    let catalog = load_capture_catalog();
    for capture in catalog.captures {
        assert!(!capture.ci_executable);
        assert_ne!(
            capture.source_tool, "gui_automation",
            "capture entries must not become live GUI automation"
        );
    }
}
