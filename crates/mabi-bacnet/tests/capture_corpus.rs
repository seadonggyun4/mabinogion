mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use support::contract::{capability, contract, peer, profile};

#[derive(Debug, Deserialize)]
struct CaptureCatalog {
    version: u32,
    captures: Vec<CaptureCatalogEntry>,
}

#[derive(Debug, Deserialize)]
struct CaptureCatalogEntry {
    id: String,
    tool: String,
    peer: String,
    lane: String,
    curation_state: String,
    profile_ids: Vec<String>,
    capability_ids: Vec<String>,
    artifact_dir: String,
    replay_kind: String,
    ci_executable: bool,
    notes: String,
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
    tool: String,
    peer: String,
    lane: String,
    curation_state: String,
    source_kind: String,
    profile_ids: Vec<String>,
    capability_ids: Vec<String>,
    replay_kind: String,
    ci_executable: bool,
    notes: String,
    capture_metadata: Option<CaptureMetadata>,
}

#[derive(Debug, Deserialize)]
struct CaptureMetadata {
    tool_version: String,
    os: String,
    bind_address: String,
    device_instance: u32,
    observed_object_tree: Vec<String>,
    failed_property: String,
    notes: String,
    artifact_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CaptureReplay {
    version: u32,
    peer: String,
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
    object: Option<String>,
    property: Option<String>,
    array_index: Option<u32>,
    value: Option<Value>,
    expect: Option<String>,
    invoke_id: Option<u32>,
    notes: Option<String>,
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

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("mabi-bacnet crate should live two levels below repo root")
        .to_path_buf()
}

fn captures_root() -> PathBuf {
    repo_root().join("verification/bacnet/captures")
}

fn capture_catalog_path() -> PathBuf {
    captures_root().join("catalog.toml")
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

#[test]
fn capture_catalog_matches_phase_4_policy_contract() {
    let verification = contract();
    assert!(verification.baseline.capture_corpus_present);
    assert_eq!(verification.policies.capture_lane, "manual_only");

    let catalog = load_capture_catalog();
    assert_eq!(catalog.version, 1);
    assert!(
        catalog.captures.len() >= 2,
        "capture catalog should include at least one seeded capture for each GUI tool lane"
    );

    let mut saw_yabe = false;
    let mut saw_vts = false;

    for capture in &catalog.captures {
        assert_eq!(capture.lane, "capture_manual");
        assert!(!capture.ci_executable);
        assert!(
            !capture.curation_state.trim().is_empty(),
            "capture {} must declare a curation_state",
            capture.id
        );
        assert!(
            !capture.replay_kind.trim().is_empty(),
            "capture {} must declare a replay_kind",
            capture.id
        );
        assert!(
            !capture.notes.trim().is_empty(),
            "capture {} must include maintainership notes",
            capture.id
        );

        let peer_contract = peer(&capture.peer);
        assert_eq!(capture.tool, capture.peer);
        assert_eq!(peer_contract.automation_lane, "capture_manual");
        assert!(peer_contract.excluded_from_current_ci);

        match capture.tool.as_str() {
            "yabe" => saw_yabe = true,
            "vts" => saw_vts = true,
            other => panic!("unexpected GUI capture tool {other}"),
        }

        for profile_id in &capture.profile_ids {
            profile(profile_id);
        }
        for capability_id in &capture.capability_ids {
            capability(capability_id);
        }
    }

    assert!(
        saw_yabe,
        "catalog should contain at least one YABE seed capture"
    );
    assert!(
        saw_vts,
        "catalog should contain at least one VTS seed capture"
    );
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
            "artifact dir for {} must stay inside verification/bacnet/captures",
            capture.id
        );

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
                    assert_eq!(artifact.format, "toml");
                    let manifest: CaptureManifest = toml::from_str(
                        &fs::read_to_string(&artifact_path).expect("manifest should read"),
                    )
                    .expect("manifest should parse");
                    assert_eq!(manifest.version, 1);
                    assert_eq!(manifest.id, capture.id);
                    assert_eq!(manifest.tool, capture.tool);
                    assert_eq!(manifest.peer, capture.peer);
                    assert_eq!(manifest.lane, capture.lane);
                    assert_eq!(manifest.profile_ids, capture.profile_ids);
                    assert_eq!(manifest.capability_ids, capture.capability_ids);
                    assert_eq!(manifest.replay_kind, capture.replay_kind);
                    assert_eq!(manifest.ci_executable, capture.ci_executable);
                    assert_eq!(manifest.curation_state, capture.curation_state);
                    assert!(
                        !manifest.source_kind.trim().is_empty(),
                        "manifest {} must declare a source kind",
                        manifest.id
                    );
                    assert!(
                        !manifest.notes.trim().is_empty(),
                        "manifest {} must explain how the seed should be maintained",
                        manifest.id
                    );
                }
                "replay" => {
                    assert_eq!(artifact.format, "json");
                    let replay: CaptureReplay = serde_json::from_str(
                        &fs::read_to_string(&artifact_path).expect("replay should read"),
                    )
                    .expect("replay json should parse");
                    assert_eq!(replay.version, 1);
                    assert_eq!(replay.peer, capture.peer);
                    assert_eq!(replay.profile_ids, capture.profile_ids);
                    assert_eq!(replay.capability_ids, capture.capability_ids);
                    assert!(
                        !replay.steps.is_empty(),
                        "replay for {} must contain at least one step",
                        capture.id
                    );
                    assert!(
                        !replay.expected_outcomes.is_empty(),
                        "replay for {} must contain expected outcomes",
                        capture.id
                    );
                    for step in &replay.steps {
                        assert!(
                            !step.id.trim().is_empty(),
                            "replay step ids must be stable for {}",
                            capture.id
                        );
                        assert!(
                            !step.kind.trim().is_empty(),
                            "replay steps must declare kind for {}",
                            capture.id
                        );
                        assert!(
                            step.request.is_some()
                                || step.object.is_some()
                                || step.property.is_some()
                                || step.value.is_some()
                                || step.expect.is_some()
                                || step.invoke_id.is_some()
                                || step.notes.is_some(),
                            "replay steps for {} must contain replay detail",
                            capture.id
                        );
                    }
                }
                "packet_summary" => {
                    assert_eq!(artifact.format, "json");
                    let summary: PacketSummary = serde_json::from_str(
                        &fs::read_to_string(&artifact_path).expect("packet summary should read"),
                    )
                    .expect("packet summary json should parse");
                    assert_eq!(summary.version, 1);
                    assert_eq!(summary.tool, capture.tool);
                    assert!(
                        !summary.packets.is_empty(),
                        "packet summary for {} must not be empty",
                        capture.id
                    );
                    for packet in &summary.packets {
                        assert!(!packet.direction.trim().is_empty());
                        assert!(!packet.service.trim().is_empty());
                        assert!(!packet.purpose.trim().is_empty());
                    }
                }
                "script" | "runbook" => {
                    let content =
                        fs::read_to_string(&artifact_path).expect("text artifact should read");
                    assert!(
                        !content.trim().is_empty(),
                        "{} artifact for {} must not be empty",
                        artifact.role,
                        capture.id
                    );
                }
                other => panic!("unsupported capture artifact role {other}"),
            }
        }
    }
}

#[test]
fn capture_lane_keeps_gui_tools_out_of_ci_execution() {
    let catalog = load_capture_catalog();
    assert_eq!(peer("yabe").automation_lane, "capture_manual");
    assert_eq!(peer("vts").automation_lane, "capture_manual");
    assert!(peer("yabe").excluded_from_current_ci);
    assert!(peer("vts").excluded_from_current_ci);

    for capture in &catalog.captures {
        assert_eq!(capture.lane, "capture_manual");
        assert!(
            !capture.ci_executable,
            "capture corpus entries must stay manual-only"
        );
    }
}

#[test]
fn yabe_empty_registry_capture_covers_phase_2_metadata_sequence() {
    let catalog = load_capture_catalog();
    let capture = catalog
        .captures
        .iter()
        .find(|capture| capture.id == "yabe-empty-registry-device-metadata")
        .expect("Phase 2 YABE empty-registry metadata capture should be cataloged");

    assert_eq!(capture.tool, "yabe");
    assert_eq!(capture.peer, "yabe");
    assert_eq!(capture.lane, "capture_manual");
    assert_eq!(capture.profile_ids, ["basic_ip"]);
    assert_eq!(capture.capability_ids, ["discovery", "property_io"]);
    assert!(!capture.ci_executable);

    let manifest_path = capture
        .artifacts
        .iter()
        .find(|artifact| artifact.role == "manifest")
        .map(|artifact| resolve_capture_path(&artifact.path))
        .expect("YABE empty-registry capture should declare a manifest");
    let manifest: CaptureManifest =
        toml::from_str(&fs::read_to_string(manifest_path).expect("manifest should read"))
            .expect("manifest should parse");
    let metadata = manifest
        .capture_metadata
        .expect("YABE empty-registry capture should declare capture metadata");
    assert!(!metadata.tool_version.trim().is_empty());
    assert!(!metadata.os.trim().is_empty());
    assert!(!metadata.bind_address.trim().is_empty());
    assert_eq!(metadata.device_instance, 4104);
    assert_eq!(metadata.observed_object_tree, ["device,4104"]);
    assert_eq!(metadata.failed_property, "none");
    assert!(!metadata.notes.trim().is_empty());
    for required_artifact in [
        "manifest.toml",
        "replay.json",
        "packet-summary.json",
        "runbook.md",
    ] {
        assert!(
            metadata
                .artifact_paths
                .iter()
                .any(|path| path == required_artifact),
            "YABE capture metadata should list {required_artifact}"
        );
    }

    let replay_path = capture
        .artifacts
        .iter()
        .find(|artifact| artifact.role == "replay")
        .map(|artifact| resolve_capture_path(&artifact.path))
        .expect("YABE empty-registry capture should declare a replay");
    let replay: CaptureReplay =
        serde_json::from_str(&fs::read_to_string(replay_path).expect("replay should read"))
            .expect("replay should parse");

    let has_who_is = replay
        .steps
        .iter()
        .any(|step| step.request.as_deref() == Some("Who-Is"));
    let has_device_object_name = replay.steps.iter().any(|step| {
        step.property.as_deref() == Some("Object_Name")
            && step.object.as_deref() == Some("device,4104")
    });
    let has_full_object_list = replay
        .steps
        .iter()
        .any(|step| step.property.as_deref() == Some("Object_List") && step.array_index.is_none());
    let has_object_list_count = replay
        .steps
        .iter()
        .any(|step| step.property.as_deref() == Some("Object_List") && step.array_index == Some(0));
    let has_object_list_first_element = replay
        .steps
        .iter()
        .any(|step| step.property.as_deref() == Some("Object_List") && step.array_index == Some(1));

    assert!(has_who_is, "YABE capture should include Who-Is discovery");
    assert!(
        has_device_object_name,
        "YABE capture should include Device Object_Name"
    );
    assert!(
        has_full_object_list,
        "YABE capture should include full Device Object_List"
    );
    assert!(
        has_object_list_count,
        "YABE capture should include Object_List index 0 count"
    );
    assert!(
        has_object_list_first_element,
        "YABE capture should include Object_List index 1 first element"
    );

    for expected_outcome in [
        "discovery_ok",
        "device_name_readable",
        "object_list_full_readable",
        "object_list_index_0_count_readable",
        "object_list_index_1_device_readable",
        "only_mandatory_device_object_expected",
    ] {
        assert!(
            replay.expected_outcomes.contains_key(expected_outcome),
            "YABE replay should declare expected outcome {expected_outcome}"
        );
    }
}
