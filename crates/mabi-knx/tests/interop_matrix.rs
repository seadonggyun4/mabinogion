use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_yaml::Value;

#[derive(Debug, Deserialize)]
struct InteropMatrix {
    version: u32,
    targets: Vec<InteropTarget>,
}

#[derive(Debug, Deserialize)]
struct InteropTarget {
    name: String,
    compose_service: String,
    peer: String,
    timeout_seconds: u64,
    tier: String,
    profiles: Vec<String>,
    capabilities: Vec<String>,
    #[serde(default)]
    working_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct VerificationContract {
    capabilities: Vec<ContractEntry>,
    profiles: Vec<ContractEntry>,
    peers: Vec<ContractEntry>,
}

#[derive(Debug, Deserialize)]
struct ContractEntry {
    id: String,
}

const CONTRACT: &str = include_str!("../../../docs/knx-simulator/verification-contract.yaml");

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn verification_dir() -> PathBuf {
    repo_root().join("verification").join("knx")
}

fn manifest_path() -> PathBuf {
    verification_dir().join("interop-matrix.toml")
}

fn compose_path() -> PathBuf {
    verification_dir().join("compose.yaml")
}

fn docker_readiness() -> Result<(), String> {
    let docker_version = Command::new("docker").arg("--version").output();
    match docker_version {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            return Err(format!("docker CLI unavailable (status {})", output.status));
        }
        Err(error) => return Err(format!("docker CLI unavailable: {error}")),
    }

    let compose_version = Command::new("docker")
        .args(["compose", "version"])
        .output()
        .map_err(|error| format!("docker compose unavailable: {error}"))?;
    if !compose_version.status.success() {
        return Err(format!(
            "docker compose unavailable (status {})",
            compose_version.status
        ));
    }

    let docker_info = Command::new("docker")
        .arg("info")
        .output()
        .map_err(|error| format!("docker daemon unavailable: {error}"))?;
    if !docker_info.status.success() {
        return Err(format!(
            "docker daemon unavailable (status {})",
            docker_info.status
        ));
    }

    Ok(())
}

fn interop_required() -> bool {
    std::env::var_os("CI").is_some() || std::env::var_os("MABI_KNX_INTEROP_REQUIRED").is_some()
}

fn manual_targets_enabled() -> bool {
    std::env::var_os("MABI_KNX_INTEROP_INCLUDE_MANUAL").is_some()
}

fn contract_ids(entries: &[ContractEntry]) -> HashSet<&str> {
    entries.iter().map(|entry| entry.id.as_str()).collect()
}

fn compose_has_service(compose: &Value, service_name: &str) -> bool {
    compose
        .get("services")
        .and_then(Value::as_mapping)
        .map(|services| {
            services
                .keys()
                .filter_map(Value::as_str)
                .any(|name| name == service_name)
        })
        .unwrap_or(false)
}

fn validate_target(target: &InteropTarget, compose: &Value, contract: &VerificationContract) {
    assert!(!target.name.is_empty(), "interop target name is empty");
    assert!(
        !target.compose_service.is_empty(),
        "interop target `{}` has an empty compose service",
        target.name
    );
    assert_eq!(
        target.name, target.compose_service,
        "interop target `{}` must match compose_service `{}` for repo-local runner safety",
        target.name, target.compose_service
    );
    assert!(
        compose_has_service(compose, &target.compose_service),
        "interop target `{}` references missing compose service `{}`",
        target.name,
        target.compose_service
    );
    assert!(
        target.timeout_seconds > 0,
        "interop target `{}` must have a positive timeout",
        target.name
    );
    assert!(
        !target.tier.is_empty(),
        "interop target `{}` must declare a tier",
        target.name
    );
    assert!(
        !target.peer.is_empty(),
        "interop target `{}` must declare a peer",
        target.name
    );
    assert!(
        !target.profiles.is_empty(),
        "interop target `{}` must declare profiles",
        target.name
    );
    assert!(
        !target.capabilities.is_empty(),
        "interop target `{}` must declare capabilities",
        target.name
    );

    let peers = contract_ids(&contract.peers);
    assert!(
        peers.contains(target.peer.as_str()),
        "interop target `{}` references unknown peer `{}`",
        target.name,
        target.peer
    );

    let profiles = contract_ids(&contract.profiles);
    for profile in &target.profiles {
        assert!(
            profiles.contains(profile.as_str()),
            "interop target `{}` references unknown profile `{profile}`",
            target.name
        );
    }

    let capabilities = contract_ids(&contract.capabilities);
    for capability in &target.capabilities {
        assert!(
            capabilities.contains(capability.as_str()),
            "interop target `{}` references unknown capability `{capability}`",
            target.name
        );
    }
}

fn target_working_dir(repo_root: &Path, target: &InteropTarget) -> PathBuf {
    target
        .working_dir
        .as_ref()
        .map(|working_dir| {
            if working_dir.is_absolute() {
                working_dir.clone()
            } else {
                repo_root.join(working_dir)
            }
        })
        .unwrap_or_else(|| repo_root.to_path_buf())
}

#[test]
#[ignore]
fn repo_local_knx_interop_matrix_executes_available_targets() {
    let matrix_content = fs::read_to_string(manifest_path()).unwrap();
    let matrix: InteropMatrix = toml::from_str(&matrix_content).unwrap();
    let compose_content = fs::read_to_string(compose_path()).unwrap();
    let compose: Value = serde_yaml::from_str(&compose_content).unwrap();
    let contract: VerificationContract = serde_yaml::from_str(CONTRACT).unwrap();

    assert_eq!(matrix.version, 1);
    assert!(!matrix.targets.is_empty());

    for target in &matrix.targets {
        validate_target(target, &compose, &contract);
    }

    let root = repo_root();
    let verification_dir = verification_dir();
    let docker_ready = docker_readiness();
    let mut passed = Vec::new();
    let mut skipped = Vec::new();
    let mut failed = Vec::new();
    let mut runnable_targets = Vec::new();

    for target in matrix.targets {
        if target.tier == "manual" && !manual_targets_enabled() {
            skipped.push(format!("{} (manual target not enabled)", target.name));
        } else {
            runnable_targets.push(target);
        }
    }

    if let Err(reason) = &docker_ready {
        if interop_required() {
            panic!("self-contained knx interop requires docker: {reason}");
        }

        skipped.extend(
            runnable_targets
                .iter()
                .map(|target| format!("{} ({reason})", target.name)),
        );

        eprintln!(
            "knx self-contained interop matrix: passed=0, failed=0, skipped={}",
            skipped.len()
        );
        eprintln!("  skipped: {}", skipped.join(", "));
        return;
    }

    for target in runnable_targets {
        let mut command = Command::new("bash");
        command.arg(verification_dir.join("run-target.sh"));
        command.arg(&target.compose_service);
        command.current_dir(target_working_dir(&root, &target));

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                failed.push(format!("{} (spawn failed: {error})", target.name));
                continue;
            }
        };

        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => {
                    if started.elapsed() >= Duration::from_secs(target.timeout_seconds) {
                        let _ = child.kill();
                        let _ = child.wait();
                        break Err(format!("timeout after {}s", target.timeout_seconds));
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(error) => break Err(format!("wait failed: {error}")),
            }
        };

        match status {
            Ok(status) if status.success() => passed.push(target.name),
            Ok(status) => failed.push(format!("{} (exit status: {status})", target.name)),
            Err(error) => failed.push(format!("{} ({error})", target.name)),
        }
    }

    eprintln!(
        "knx self-contained interop matrix: passed={}, failed={}, skipped={}",
        passed.len(),
        failed.len(),
        skipped.len()
    );
    if !passed.is_empty() {
        eprintln!("  passed: {}", passed.join(", "));
    }
    if !skipped.is_empty() {
        eprintln!("  skipped: {}", skipped.join(", "));
    }

    assert!(
        failed.is_empty(),
        "knx self-contained interop matrix failures: {}",
        failed.join(", ")
    );
}
