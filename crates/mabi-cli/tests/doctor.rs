use assert_cmd::assert::OutputAssertExt;
use serde_json::Value;
use std::process::Command;

fn mabi() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mabi"))
}

#[test]
fn doctor_runs_all_builtin_protocol_smokes() {
    let mut command = mabi();
    command
        .args(["--no-color", "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("mabi doctor"))
        .stdout(predicates::str::contains("modbus"))
        .stdout(predicates::str::contains("opcua"))
        .stdout(predicates::str::contains("bacnet"))
        .stdout(predicates::str::contains("knx"));
}

#[test]
fn doctor_protocol_filter_limits_json_report() {
    let output = mabi()
        .args(["--format", "json", "doctor", "--protocol", "modbus"])
        .output()
        .expect("doctor command should run");

    assert!(
        output.status.success(),
        "doctor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("doctor output is JSON");
    let protocols = report["protocols"].as_array().expect("protocols array");
    assert_eq!(protocols.len(), 1);
    assert_eq!(protocols[0]["protocol"], "modbus");
    assert_eq!(protocols[0]["launch_ok"], true);
    assert_eq!(protocols[0]["ready_ok"], true);
    assert_eq!(protocols[0]["snapshot_ok"], true);
    assert_eq!(protocols[0]["stop_ok"], true);
}

#[test]
fn doctor_optional_interop_prereqs_do_not_fail_install_smoke() {
    let output = mabi()
        .args(["--format", "json", "doctor"])
        .output()
        .expect("doctor command should run");

    assert!(
        output.status.success(),
        "doctor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("doctor output is JSON");
    let optional = report["optional_prereqs"]
        .as_array()
        .expect("optional prereqs array");
    assert!(optional.iter().all(|entry| entry["status"] == "skip"));
    assert!(optional.iter().any(|entry| entry["id"] == "interop.docker"));
}
