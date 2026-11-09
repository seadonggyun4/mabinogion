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
        .stdout(contains(mabi_core::RELEASE_VERSION));
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
        "unified-readiness-contract-v2"
    );
    assert_eq!(
        envelope["data"]["contract_versions"]["run_evidence_schema"],
        "run-evidence-schema-v2"
    );
    assert_eq!(
        envelope["data"]["contract_versions"]["replay_artifact_contract"],
        "replay-artifact-contract-v1"
    );
    assert_eq!(
        envelope["data"]["contract_versions"]["version_metadata_contract"],
        "version-metadata-contract-v2"
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
        "compatibility-matrix-v2"
    );
    assert_eq!(
        envelope["data"]["runner_metadata"]["compatibility_decision_owner"],
        "external-consumer"
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
            "protocol-readiness-matrix-v2"
        );
        assert_eq!(
            protocol["readiness_contract_version"],
            "unified-readiness-contract-v2"
        );
        assert!(
            !protocol["profile_ids"]
                .as_array()
                .expect("profile ids should be an array")
                .is_empty(),
            "protocol should expose profile ids"
        );
    }
}
