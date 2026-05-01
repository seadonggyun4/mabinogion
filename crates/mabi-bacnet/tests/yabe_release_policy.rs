mod support;

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use support::contract::{contract, peer};

#[derive(Debug, Deserialize)]
struct CaptureCatalog {
    captures: Vec<CaptureCatalogEntry>,
}

#[derive(Debug, Deserialize)]
struct CaptureCatalogEntry {
    id: String,
    peer: String,
    lane: String,
    ci_executable: bool,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("mabi-bacnet crate should live two levels below repo root")
        .to_path_buf()
}

fn read_repo_file(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"))
}

fn assert_ignored_test(source: &str, function_name: &str) {
    let lines: Vec<_> = source.lines().collect();
    let function_line = lines
        .iter()
        .position(|line| line.contains(&format!("fn {function_name}")))
        .unwrap_or_else(|| panic!("missing interop profile {function_name}"));
    let attrs = lines[function_line.saturating_sub(8)..function_line].join("\n");
    assert!(
        attrs.contains("#[ignore]"),
        "{function_name} must stay in the ignored interop lane"
    );
}

#[test]
fn yabe_phase5_document_declares_acceptance_release_and_heavy_tool_boundaries() {
    let plan = read_repo_file("docs/bacnet-simulator/yabe-discovery-compatibility-plan.md");

    for task in [
        "phase5.acceptance_matrix",
        "phase5.release_notes",
        "phase5.no_default_heavy_tools",
    ] {
        assert!(plan.contains(task), "Phase 5 plan missing {task}");
    }

    assert!(plan.contains("Acceptance Matrix"));
    assert!(plan.contains("Release Note Requirement"));
    assert!(plan.contains(
        "Improved BACnet explorer/YABE compatibility for empty-registry Device metadata discovery."
    ));
    assert!(plan.contains("Docker or Docker Compose"));
    assert!(plan.contains("YABE or other GUI tools"));
    assert!(plan.contains("external BACnet peer processes"));
    assert!(plan.contains("threshold-based perf assertions"));
    assert!(plan.contains("cargo test --workspace"));
}

#[test]
fn yabe_manual_capture_and_surrogate_interop_stay_out_of_default_lane() {
    let verification = contract();
    assert_eq!(
        verification.policies.default_workspace_lane,
        "deterministic"
    );
    assert_eq!(verification.policies.interop_lane, "ignored");
    assert_eq!(verification.policies.capture_lane, "manual_only");
    assert_eq!(verification.policies.perf_lane, "release_only_ignored");
    assert!(verification.policies.default_perf_thresholds_forbidden);
    assert_eq!(verification.policies.gui_automation, "out_of_scope");

    let yabe = peer("yabe");
    assert_eq!(yabe.automation_lane, "capture_manual");
    assert!(yabe.excluded_from_current_ci);

    let catalog: CaptureCatalog =
        toml::from_str(&read_repo_file("verification/bacnet/captures/catalog.toml"))
            .expect("capture catalog should parse");
    let yabe_empty_registry = catalog
        .captures
        .iter()
        .find(|capture| capture.id == "yabe-empty-registry-device-metadata")
        .expect("YABE empty-registry capture should stay cataloged");
    assert_eq!(yabe_empty_registry.peer, "yabe");
    assert_eq!(yabe_empty_registry.lane, "capture_manual");
    assert!(
        !yabe_empty_registry.ci_executable,
        "YABE capture artifacts must never become default-lane executable tests"
    );

    let interop_profiles = read_repo_file("crates/mabi-bacnet/tests/interop_profiles.rs");
    assert_ignored_test(&interop_profiles, "bacpypes3_yabe_sequence_smoke_contract");
    assert_ignored_test(
        &interop_profiles,
        "bac0_yabe_readmultiple_probe_smoke_contract",
    );

    let matrix = read_repo_file("verification/bacnet/interop-matrix.toml");
    assert!(matrix.contains("bacpypes3-yabe-sequence"));
    assert!(matrix.contains("bac0-yabe-readmultiple"));
}

#[test]
fn bacnet_verification_readmes_match_phase5_release_policy() {
    let simulator_readme = read_repo_file("docs/bacnet-simulator/README.md");
    let verification_readme = read_repo_file("verification/bacnet/README.md");

    for document in [&simulator_readme, &verification_readme] {
        assert!(document.contains("cargo test --workspace"));
        assert!(document.contains("YABE"));
        assert!(document.contains("manual"));
        assert!(document.contains("ignored"));
        assert!(document.contains("perf"));
        assert!(document.contains("Docker"));
        assert!(document.contains("GUI"));
    }

    assert!(verification_readme.contains("Release Policy"));
    assert!(verification_readme.contains("improved BACnet explorer/YABE compatibility for"));
}
