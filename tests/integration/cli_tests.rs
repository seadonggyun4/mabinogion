//! CLI integration tests.
//!
//! Tests for the CLI binary using assert_cmd.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn otsim_cmd() -> Command {
    Command::cargo_bin("otsim").unwrap()
}

#[test]
fn test_version_command() {
    otsim_cmd()
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("otsim"));
}

#[test]
fn test_help_command() {
    otsim_cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Industrial protocol simulator"));
}

#[test]
fn test_list_protocols() {
    otsim_cmd()
        .args(["list", "protocols"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Modbus TCP"))
        .stdout(predicate::str::contains("OPC UA"))
        .stdout(predicate::str::contains("BACnet"))
        .stdout(predicate::str::contains("KNX"));
}

#[test]
fn test_list_protocols_json() {
    otsim_cmd()
        .args(["--format", "json", "list", "protocols"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"protocol\""));
}

#[test]
fn test_validate_missing_file() {
    otsim_cmd()
        .args(["validate", "nonexistent.yaml"])
        .assert()
        .failure();
}

#[test]
fn test_validate_valid_scenario() {
    let temp_dir = TempDir::new().unwrap();
    let scenario_path = temp_dir.path().join("test_scenario.yaml");

    fs::write(
        &scenario_path,
        r#"
name: Test Scenario
description: A test scenario

devices:
  - id: test-device
    protocol: modbus_tcp
    name: Test Device
    points:
      - id: point-1
        data_type: uint16
        initial_value: 0
"#,
    )
    .unwrap();

    otsim_cmd()
        .args(["validate", scenario_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Valid"));
}

#[test]
fn test_validate_invalid_yaml() {
    let temp_dir = TempDir::new().unwrap();
    let scenario_path = temp_dir.path().join("invalid.yaml");

    fs::write(&scenario_path, "name: [invalid yaml").unwrap();

    otsim_cmd()
        .args(["validate", scenario_path.to_str().unwrap()])
        .assert()
        .failure();
}

#[test]
fn test_validate_empty_scenario() {
    let temp_dir = TempDir::new().unwrap();
    let scenario_path = temp_dir.path().join("empty.yaml");

    fs::write(
        &scenario_path,
        r#"
name: ""
devices: []
"#,
    )
    .unwrap();

    otsim_cmd()
        .args(["validate", "--strict", scenario_path.to_str().unwrap()])
        .assert()
        .failure();
}

#[test]
fn test_validate_multiple_files() {
    let temp_dir = TempDir::new().unwrap();

    let valid_path = temp_dir.path().join("valid.yaml");
    fs::write(
        &valid_path,
        r#"
name: Valid
devices:
  - id: dev1
    protocol: modbus_tcp
    points: []
"#,
    )
    .unwrap();

    let invalid_path = temp_dir.path().join("invalid.yaml");
    fs::write(&invalid_path, "not: valid: yaml: [").unwrap();

    otsim_cmd()
        .args([
            "validate",
            valid_path.to_str().unwrap(),
            invalid_path.to_str().unwrap(),
        ])
        .assert()
        .failure();
}

#[test]
fn test_modbus_help() {
    otsim_cmd()
        .args(["modbus", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Modbus TCP/RTU"));
}

#[test]
fn test_opcua_help() {
    otsim_cmd()
        .args(["opcua", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("OPC UA"));
}

#[test]
fn test_bacnet_help() {
    otsim_cmd()
        .args(["bacnet", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BACnet"));
}

#[test]
fn test_knx_help() {
    otsim_cmd()
        .args(["knx", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("KNX"));
}

#[test]
fn test_run_dry_run() {
    let temp_dir = TempDir::new().unwrap();
    let scenario_path = temp_dir.path().join("scenario.yaml");

    fs::write(
        &scenario_path,
        r#"
name: Dry Run Test
description: Test dry run mode

devices:
  - id: device-1
    protocol: modbus_tcp
    name: Device 1
    points:
      - id: point-1
        data_type: uint16
"#,
    )
    .unwrap();

    otsim_cmd()
        .args(["run", "--dry-run", scenario_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run completed"));
}

#[test]
fn test_run_missing_scenario() {
    otsim_cmd()
        .args(["run", "nonexistent.yaml"])
        .assert()
        .failure();
}

#[test]
fn test_quiet_mode() {
    otsim_cmd()
        .args(["--quiet", "list", "protocols"])
        .assert()
        .success();
}

#[test]
fn test_verbose_mode() {
    otsim_cmd()
        .args(["-v", "list", "protocols"])
        .assert()
        .success();
}

#[test]
fn test_no_color_mode() {
    otsim_cmd()
        .args(["--no-color", "list", "protocols"])
        .assert()
        .success();
}

#[test]
fn test_list_devices_empty() {
    otsim_cmd()
        .args(["list", "devices"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No devices found"));
}

#[test]
fn test_list_scenarios_empty() {
    let temp_dir = TempDir::new().unwrap();

    otsim_cmd()
        .current_dir(&temp_dir)
        .args(["list", "scenarios"])
        .assert()
        .success();
}
