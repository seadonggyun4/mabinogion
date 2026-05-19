use assert_cmd::assert::OutputAssertExt;
use predicates::str::contains;
use serde_json::Value;
use std::process::Command;

#[test]
fn version_command_reports_workspace_release_version() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mabi"));
    command
        .arg("version")
        .assert()
        .success()
        .stdout(contains(format!(
            "mabi {} (Mabinogion)",
            mabi_core::RELEASE_VERSION
        )));
}

#[test]
fn version_command_reports_runner_contract_metadata_as_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_mabi"))
        .args(["--format", "json", "version"])
        .output()
        .expect("version command should run");

    assert!(
        output.status.success(),
        "version failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("version output is JSON");
    assert_eq!(envelope["contract_version"], "local-runner-contract-v1");
    assert_eq!(envelope["envelope_version"], "cli-output-envelope-v1");
    assert_eq!(envelope["command"], "version");
    assert_eq!(envelope["status"], "success");
    assert_eq!(
        envelope["data"]["engine_version"],
        mabi_core::RELEASE_VERSION
    );
    assert_eq!(
        envelope["data"]["contract_versions"]["local_runner_contract"],
        "local-runner-contract-v1"
    );
    assert_eq!(
        envelope["data"]["contract_versions"]["runtime_contract"],
        "runtime-contract-v1"
    );
    assert_eq!(
        envelope["data"]["contract_versions"]["unified_readiness_contract"],
        "unified-readiness-contract-v1"
    );
    assert_eq!(
        envelope["data"]["contract_versions"]["run_evidence_schema"],
        "run-evidence-schema-v1"
    );
    assert_eq!(
        envelope["data"]["contract_versions"]["trial_artifact_contract"],
        "trial-artifact-contract-v1"
    );
    assert_eq!(
        envelope["data"]["contract_versions"]["version_metadata_contract"],
        "version-metadata-contract-v1"
    );
    assert_eq!(
        envelope["data"]["release_metadata"]["engine_version"],
        mabi_core::RELEASE_VERSION
    );
    assert_eq!(
        envelope["data"]["release_metadata"]["release_channel"],
        "source-build"
    );
    assert_eq!(
        envelope["data"]["release_metadata"]["compatibility_matrix_version"],
        "compatibility-matrix-v1"
    );
    assert_eq!(
        envelope["data"]["trial_compatible_metadata"]["trial_definition_owner"],
        "mabinogion-trials"
    );
    assert_eq!(
        envelope["data"]["trial_compatible_metadata"]["compatible_trial_suite_range"],
        ">=0.1.0 <1.0.0"
    );
    assert_eq!(
        envelope["data"]["trial_compatible_metadata"]["compatibility_decision_owner"],
        "mabinogion-trials-or-forge"
    );
    assert_eq!(
        envelope["data"]["registered_protocols"]
            .as_array()
            .expect("protocols array")
            .len(),
        4
    );
    for protocol in envelope["data"]["registered_protocols"]
        .as_array()
        .expect("protocols array")
    {
        assert_eq!(protocol["protocol_key"], protocol["key"]);
        assert!(
            protocol["capability_version"]
                .as_str()
                .expect("capability version should be a string")
                .ends_with("-capabilities-v1"),
            "protocol should report a capability version"
        );
        assert_eq!(
            protocol["readiness_matrix_version"],
            "protocol-readiness-matrix-v1"
        );
        assert_eq!(
            protocol["readiness_contract_version"],
            "unified-readiness-contract-v1"
        );
        assert!(
            !protocol["trial_profile_ids"]
                .as_array()
                .expect("trial profile ids should be an array")
                .is_empty(),
            "protocol should expose trial profile ids"
        );
    }
}
