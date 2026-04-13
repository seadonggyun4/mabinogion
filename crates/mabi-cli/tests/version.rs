use assert_cmd::assert::OutputAssertExt;
use predicates::str::contains;
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
