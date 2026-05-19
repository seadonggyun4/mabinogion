use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Debug, Deserialize)]
struct LocalRunnerContract {
    schema_version: u16,
    contract_version: String,
    envelope_version: String,
    ownership_boundary: OwnershipBoundary,
    machine_output: MachineOutput,
    exit_code_policy: Vec<ExitCodePolicy>,
    runner_facing_commands: Vec<RunnerFacingCommand>,
    contract_versions_reported_by_version: Vec<String>,
    future_trial_run_execution_spec: FutureTrialRunExecutionSpec,
}

#[derive(Debug, Deserialize)]
struct OwnershipBoundary {
    mabinogion_owns: Vec<String>,
    not_owned_by_mabinogion: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MachineOutput {
    formats: Vec<String>,
    required_envelope_fields: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ExitCodePolicy {
    code: i32,
    category: String,
}

#[derive(Debug, Deserialize)]
struct RunnerFacingCommand {
    command: String,
    stable_machine_output: bool,
}

#[derive(Debug, Deserialize)]
struct FutureTrialRunExecutionSpec {
    command_status: String,
    command: String,
    owner_of_trial_definition: String,
    evidence_output_contract: String,
    artifact_contract: String,
    required_fields: Vec<String>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root should resolve")
}

fn load_contract() -> LocalRunnerContract {
    let path = repo_root().join("docs/cli/local-runner-contract.yaml");
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_yaml::from_str(&contents).expect("local runner contract YAML should parse")
}

fn mabi(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mabi"))
        .args(args)
        .output()
        .expect("mabi command should run")
}

fn parse_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout should be JSON: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn local_runner_contract_yaml_defines_runner_boundary_and_envelope() {
    let contract = load_contract();

    assert_eq!(contract.schema_version, 1);
    assert_eq!(contract.contract_version, "local-runner-contract-v1");
    assert_eq!(contract.envelope_version, "cli-output-envelope-v1");
    assert!(contract
        .ownership_boundary
        .mabinogion_owns
        .iter()
        .any(|item| item == "machine_readable_runner_output"));
    assert!(contract
        .ownership_boundary
        .not_owned_by_mabinogion
        .iter()
        .any(|item| item == "certification_issuance"));

    let formats = contract
        .machine_output
        .formats
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    assert_eq!(formats, HashSet::from(["json", "yaml", "compact"]));

    for field in [
        "contract_version",
        "envelope_version",
        "command",
        "status",
        "exit_code",
        "exit_category",
        "generated_at",
        "engine_version",
        "data",
        "warnings",
        "errors",
    ] {
        assert!(
            contract
                .machine_output
                .required_envelope_fields
                .iter()
                .any(|documented| documented == field),
            "envelope field {field} should be documented"
        );
    }
}

#[test]
fn local_runner_contract_yaml_defines_exit_codes_and_commands() {
    let contract = load_contract();

    let exit_codes = contract
        .exit_code_policy
        .iter()
        .map(|entry| (entry.code, entry.category.as_str()))
        .collect::<HashMap<_, _>>();
    assert_eq!(exit_codes.get(&0), Some(&"success"));
    assert_eq!(exit_codes.get(&2), Some(&"input_contract_error"));
    assert_eq!(exit_codes.get(&6), Some(&"validation_failure"));
    assert_eq!(exit_codes.get(&9), Some(&"runtime_failure"));
    assert_eq!(exit_codes.get(&124), Some(&"timeout"));
    assert_eq!(exit_codes.get(&130), Some(&"interrupted"));
    assert_eq!(exit_codes.get(&1), Some(&"internal_failure"));

    let commands = contract
        .runner_facing_commands
        .iter()
        .map(|entry| {
            assert!(
                entry.stable_machine_output,
                "{} should be stable machine output",
                entry.command
            );
            entry.command.as_str()
        })
        .collect::<HashSet<_>>();
    for command in [
        "doctor",
        "inspect protocols",
        "inspect schema",
        "inspect status",
        "inspect modbus-config",
        "inspect opcua-config",
        "validate scenario",
        "validate config",
        "validate modbus-config",
        "validate opcua-config",
        "version",
    ] {
        assert!(
            commands.contains(command),
            "{command} should be runner-facing"
        );
    }

    for version in [
        "local-runner-contract-v1",
        "cli-output-envelope-v1",
        "runtime-contract-v1",
        "snapshot-metadata-v1",
        "unified-readiness-contract-v1",
        "run-evidence-schema-v1",
        "trial-artifact-contract-v1",
        "version-metadata-contract-v1",
    ] {
        assert!(
            contract
                .contract_versions_reported_by_version
                .iter()
                .any(|documented| documented == version),
            "{version} should be reported by mabi version"
        );
    }
}

#[test]
fn future_trial_run_contract_is_documented_without_claiming_ownership() {
    let contract = load_contract();
    let spec = contract.future_trial_run_execution_spec;

    assert_eq!(spec.command_status, "documented_future_contract_only");
    assert_eq!(spec.command, "mabi trial run");
    assert_eq!(spec.owner_of_trial_definition, "mabinogion-trials");
    assert_eq!(spec.evidence_output_contract, "run-evidence-schema-v1");
    assert_eq!(spec.artifact_contract, "trial-artifact-contract-v1");
    for field in [
        "execution_spec_version",
        "run_id",
        "trial_suite_version",
        "protocol",
        "profile_id",
        "config_path",
        "session",
        "readiness_timeout_ms",
        "artifact_dir",
        "output_format",
        "evidence_requirements",
    ] {
        assert!(
            spec.required_fields
                .iter()
                .any(|documented| documented == field),
            "future trial run field {field} should be documented"
        );
    }
}

#[test]
fn inspect_protocols_json_uses_local_runner_envelope() {
    let output = mabi(&["--format", "json", "inspect", "protocols"]);
    assert!(
        output.status.success(),
        "inspect protocols failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope = parse_stdout(&output);

    assert_eq!(envelope["contract_version"], "local-runner-contract-v1");
    assert_eq!(envelope["envelope_version"], "cli-output-envelope-v1");
    assert_eq!(envelope["command"], "inspect protocols");
    assert_eq!(envelope["exit_code"], 0);
    assert_eq!(envelope["exit_category"], "success");
    assert_eq!(
        envelope["data"]["protocols"]
            .as_array()
            .expect("protocols should be an array")
            .len(),
        4
    );
    assert_eq!(
        envelope["data"]["contract_versions"]["unified_readiness_contract"],
        "unified-readiness-contract-v1"
    );
}

#[test]
fn inspect_config_json_failure_is_enveloped() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let invalid_path = temp.path().join("invalid-modbus.yaml");
    fs::write(&invalid_path, "{").expect("invalid fixture should be written");

    let output = mabi(&[
        "--format",
        "json",
        "inspect",
        "modbus-config",
        invalid_path
            .to_str()
            .expect("invalid fixture path should be UTF-8"),
    ]);
    assert_eq!(output.status.code(), Some(2));
    let envelope = parse_stdout(&output);
    assert_eq!(envelope["contract_version"], "local-runner-contract-v1");
    assert_eq!(envelope["command"], "inspect modbus-config");
    assert_eq!(envelope["status"], "failure");
    assert_eq!(envelope["exit_code"], 2);
    assert_eq!(envelope["exit_category"], "input_contract_error");
    assert!(!envelope["errors"]
        .as_array()
        .expect("errors should be an array")
        .is_empty());
}

#[test]
fn validate_config_json_success_and_failure_are_enveloped() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let valid_path = temp.path().join("valid.yaml");
    fs::write(&valid_path, "answer: 42\n").expect("valid fixture should be written");

    let valid_output = mabi(&[
        "--format",
        "json",
        "validate",
        "config",
        valid_path
            .to_str()
            .expect("valid fixture path should be UTF-8"),
    ]);
    assert!(
        valid_output.status.success(),
        "validate config valid failed: {}",
        String::from_utf8_lossy(&valid_output.stderr)
    );
    let valid = parse_stdout(&valid_output);
    assert_eq!(valid["contract_version"], "local-runner-contract-v1");
    assert_eq!(valid["command"], "validate config");
    assert_eq!(valid["status"], "success");
    assert_eq!(valid["data"]["total_files"], 1);
    assert_eq!(valid["data"]["valid_files"], 1);

    let invalid_path = temp.path().join("invalid.json");
    fs::write(&invalid_path, "{").expect("invalid fixture should be written");
    let invalid_output = mabi(&[
        "--format",
        "json",
        "validate",
        "config",
        invalid_path
            .to_str()
            .expect("invalid fixture path should be UTF-8"),
    ]);
    assert_eq!(invalid_output.status.code(), Some(6));
    let invalid = parse_stdout(&invalid_output);
    assert_eq!(invalid["contract_version"], "local-runner-contract-v1");
    assert_eq!(invalid["command"], "validate config");
    assert_eq!(invalid["status"], "failure");
    assert_eq!(invalid["exit_code"], 6);
    assert_eq!(invalid["exit_category"], "validation_failure");
    assert!(!invalid["errors"]
        .as_array()
        .expect("errors should be an array")
        .is_empty());
}
