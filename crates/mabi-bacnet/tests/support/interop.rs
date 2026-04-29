use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivePeerTranscript {
    pub peer: String,
    pub sut_addr: String,
    pub device_instance: u32,
    #[serde(default)]
    pub discovery_ok: bool,
    #[serde(default)]
    pub read_ok: bool,
    #[serde(default)]
    pub write_ok: bool,
    #[serde(default)]
    pub property_multiple_ok: bool,
    #[serde(default)]
    pub round_trip_value: f64,
    pub errors: Vec<String>,
}

pub type Bacpypes3CanaryTranscript = ActivePeerTranscript;

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate directory should have a parent")
        .parent()
        .expect("workspace should have a parent")
        .to_path_buf()
}

pub fn verification_bacnet_dir() -> PathBuf {
    repo_root().join("verification").join("bacnet")
}

pub fn bacpypes3_peer_script() -> PathBuf {
    verification_bacnet_dir()
        .join("harness")
        .join("bacpypes3")
        .join("peer_client.py")
}

pub fn bac0_peer_script() -> PathBuf {
    verification_bacnet_dir()
        .join("harness")
        .join("bac0")
        .join("peer_client.py")
}

pub fn bacnet_stack_peer_script() -> PathBuf {
    verification_bacnet_dir()
        .join("harness")
        .join("bacnet-stack")
        .join("peer_client.sh")
}

pub fn bacnet4j_peer_script() -> PathBuf {
    verification_bacnet_dir()
        .join("harness")
        .join("bacnet4j")
        .join("peer_client.sh")
}

pub fn temp_transcript_path(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{unique}.json"))
}

pub fn load_active_peer_transcript(path: &Path) -> ActivePeerTranscript {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("interop transcript should be readable: {error}"));
    serde_json::from_str(&content)
        .unwrap_or_else(|error| panic!("interop transcript should be valid json: {error}"))
}

pub fn load_bacpypes3_transcript(path: &Path) -> Bacpypes3CanaryTranscript {
    load_active_peer_transcript(path)
}

pub async fn run_python_peer(
    script: &Path,
    envs: &[(&str, String)],
    timeout_budget: Duration,
) -> Output {
    let python = std::env::var("MABI_BACNET_INTEROP_PYTHON")
        .unwrap_or_else(|_| "python3".to_string());

    let mut command = Command::new(&python);
    command.arg(script);
    command.kill_on_drop(true);
    command.env("PYTHONUNBUFFERED", "1");
    for (key, value) in envs {
        command.env(key, value);
    }

    match timeout(timeout_budget, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => panic!("failed to run interop peer process: {error}"),
        Err(_) => panic!(
            "interop peer process timed out after {}s",
            timeout_budget.as_secs()
        ),
    }
}

pub async fn run_shell_peer(
    script: &Path,
    envs: &[(&str, String)],
    timeout_budget: Duration,
) -> Output {
    let mut command = Command::new("bash");
    command.arg(script);
    command.kill_on_drop(true);
    command.env("PYTHONUNBUFFERED", "1");
    for (key, value) in envs {
        command.env(key, value);
    }

    match timeout(timeout_budget, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => panic!("failed to run interop peer process: {error}"),
        Err(_) => panic!(
            "interop peer process timed out after {}s",
            timeout_budget.as_secs()
        ),
    }
}
