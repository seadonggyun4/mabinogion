use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct InteropMatrix {
    version: u32,
    targets: Vec<InteropTarget>,
}

#[derive(Debug, Deserialize)]
struct InteropTarget {
    name: String,
    compose_service: String,
    timeout_seconds: u64,
    tier: String,
    #[serde(default)]
    working_dir: Option<PathBuf>,
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("verification")
        .join("opcua")
        .join("interop-matrix.toml")
}

fn repo_root(manifest_dir: &Path) -> PathBuf {
    manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
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

#[test]
#[ignore]
fn repo_local_compose_interop_matrix_executes_available_targets() {
    let path = manifest_path();
    let content = fs::read_to_string(&path).unwrap();
    let matrix: InteropMatrix = toml::from_str(&content).unwrap();

    assert_eq!(matrix.version, 1);
    assert!(!matrix.targets.is_empty());

    let manifest_dir = path.parent().unwrap().to_path_buf();
    let repo_root = repo_root(&manifest_dir);
    let docker_ready = docker_readiness();
    let mut passed = Vec::new();
    let mut skipped = Vec::new();
    let mut failed = Vec::new();

    if let Err(reason) = &docker_ready {
        if std::env::var_os("CI").is_some() {
            panic!("self-contained opcua interop requires docker in CI: {reason}");
        }

        for target in &matrix.targets {
            skipped.push(format!("{} ({reason})", target.name));
        }

        eprintln!(
            "opcua self-contained interop matrix: passed=0, failed=0, skipped={}",
            skipped.len()
        );
        eprintln!("  skipped: {}", skipped.join(", "));
        return;
    }

    for target in matrix.targets {
        assert!(!target.name.is_empty());
        assert!(!target.compose_service.is_empty());
        assert!(target.timeout_seconds > 0);
        assert!(!target.tier.is_empty());

        let mut command = Command::new("bash");
        command.arg(manifest_dir.join("run-target.sh"));
        command.arg(&target.compose_service);
        command.current_dir(
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
                .unwrap_or_else(|| repo_root.clone()),
        );

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
        "opcua self-contained interop matrix: passed={}, failed={}, skipped={}",
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
        "opcua self-contained interop matrix failures: {}",
        failed.join(", ")
    );
}
