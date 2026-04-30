use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use tokio::process::Command;
use tokio::time::timeout;

use super::TestResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerFailureKind {
    ToolMissing,
    BuildFailure,
    ProtocolFailure,
    UnsupportedFeature,
    Timeout,
}

impl PeerFailureKind {
    pub fn parse(value: &str) -> TestResult<Self> {
        match value {
            "tool_missing" => Ok(Self::ToolMissing),
            "build_failure" => Ok(Self::BuildFailure),
            "protocol_failure" => Ok(Self::ProtocolFailure),
            "unsupported_feature" => Ok(Self::UnsupportedFeature),
            "timeout" => Ok(Self::Timeout),
            other => Err(format!("unknown KNX peer failure category `{other}`").into()),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PeerTranscript {
    pub schema_version: u32,
    pub target: String,
    pub peer: String,
    pub sut_addr: String,
    pub capabilities: Vec<CapabilityResult>,
    pub steps: Vec<StepResult>,
    pub failure_category: Option<String>,
    pub errors: Vec<String>,
    #[serde(default)]
    pub artifacts: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CapabilityResult {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub details: String,
}

#[derive(Debug, Deserialize)]
pub struct StepResult {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub details: String,
}

impl PeerTranscript {
    pub fn failure_kind(&self) -> TestResult<Option<PeerFailureKind>> {
        self.failure_category
            .as_deref()
            .map(PeerFailureKind::parse)
            .transpose()
    }

    pub fn capability_status(&self, capability_id: &str) -> Option<&str> {
        self.capabilities
            .iter()
            .find(|capability| capability.id == capability_id)
            .map(|capability| capability.status.as_str())
    }

    pub fn artifact_u8(&self, key: &str) -> Option<u8> {
        self.artifacts
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
    }
}

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

pub fn peer_script(peer_dir: &str, script_name: &str) -> PathBuf {
    repo_root()
        .join("verification")
        .join("knx")
        .join("harness")
        .join(peer_dir)
        .join(script_name)
}

pub fn temp_transcript_path(target: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::env::temp_dir().join(format!(
        "mabi-knx-{target}-{}-{millis}.json",
        std::process::id()
    ))
}

pub async fn run_peer_command(
    program: &str,
    args: &[PathBuf],
    envs: &[(&'static str, String)],
    peer_timeout: Duration,
) -> TestResult<Output> {
    let mut command = Command::new(program);
    for arg in args {
        command.arg(arg);
    }
    command.env_clear();
    command.env("PATH", std::env::var("PATH").unwrap_or_default());
    command.env("PYTHONUNBUFFERED", "1");
    if let Ok(node_path) = std::env::var("NODE_PATH") {
        command.env("NODE_PATH", node_path);
    }
    if let Ok(calimero_jar) = std::env::var("CALIMERO_TOOLS_JAR") {
        command.env("CALIMERO_TOOLS_JAR", calimero_jar);
    }
    if let Ok(knxd_version) = std::env::var("KNXD_EXPECTED_VERSION") {
        command.env("KNXD_EXPECTED_VERSION", knxd_version);
    }
    if let Ok(openknx_mode) = std::env::var("OPENKNX_THELSING_MODE") {
        command.env("OPENKNX_THELSING_MODE", openknx_mode);
    }
    for (name, value) in envs {
        command.env(name, value);
    }

    let child = command.output();
    match timeout(peer_timeout, child).await {
        Ok(output) => Ok(output?),
        Err(_) => Err(format!("KNX peer timed out after {}s", peer_timeout.as_secs()).into()),
    }
}

pub fn load_peer_transcript(path: &Path) -> TestResult<PeerTranscript> {
    let content = fs::read_to_string(path)?;
    let transcript: PeerTranscript = serde_json::from_str(&content)?;
    if transcript.schema_version != 1 {
        return Err(format!(
            "unsupported KNX peer transcript schema version {}",
            transcript.schema_version
        )
        .into());
    }
    let _ = transcript.failure_kind()?;
    Ok(transcript)
}
