mod support;

use std::fs;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use mabi_knx::{GroupAddress, KnxServerConfig};

use support::contract::{assert_peer_lane, assert_policy, assert_profile_lane};
use support::fixtures::{standard_group_table, SCALING};
use support::interop::{
    load_peer_transcript, peer_script, run_peer_command, temp_transcript_path, PeerTranscript,
};
use support::server_harness::ServerHarness;
use support::TestResult;
use tokio::sync::Mutex;

const KNX_INTEROP_PORT: u16 = 3_671;
const KNX_WRITE_VALUE: u8 = 42;
const PEER_TIMEOUT: Duration = Duration::from_secs(60);

static INTEROP_PORT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

struct PeerProfile {
    target: &'static str,
    peer: &'static str,
    peer_lane: &'static str,
    profiles: &'static [&'static str],
    program: &'static str,
    script: PathBuf,
    required_passed: &'static [&'static str],
    allowed_unsupported: &'static [&'static str],
    expects_group_round_trip: bool,
}

fn transcript_debug_hint(transcript_path: &Path) -> String {
    if transcript_path.exists() {
        format!(
            "\ntranscript:\n{}",
            fs::read_to_string(transcript_path)
                .unwrap_or_else(|_| "<unreadable transcript>".to_string())
        )
    } else {
        "\ntranscript:\n<missing>".to_string()
    }
}

fn assert_process_success(
    profile: &PeerProfile,
    transcript_path: &Path,
    output: &std::process::Output,
) -> TestResult {
    if output.status.success() {
        return Ok(());
    }

    Err(format!(
        "{} interop peer exited with status {}\nstdout:\n{}\nstderr:\n{}{}",
        profile.target,
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        transcript_debug_hint(transcript_path),
    )
    .into())
}

fn assert_peer_transcript(
    transcript: &PeerTranscript,
    profile: &PeerProfile,
    expected_sut_addr: &str,
) -> TestResult {
    if transcript.target != profile.target {
        return Err(format!(
            "unexpected target `{}`, expected `{}`",
            transcript.target, profile.target
        )
        .into());
    }
    if transcript.peer != profile.peer {
        return Err(format!(
            "unexpected peer `{}`, expected `{}`",
            transcript.peer, profile.peer
        )
        .into());
    }
    if transcript.sut_addr != expected_sut_addr {
        return Err(format!(
            "unexpected SUT address `{}`, expected `{expected_sut_addr}`",
            transcript.sut_addr
        )
        .into());
    }
    if let Some(kind) = transcript.failure_kind()? {
        return Err(format!("peer reported failure category: {kind:?}").into());
    }
    if !transcript.errors.is_empty() {
        return Err(format!("peer errors: {}", transcript.errors.join(", ")).into());
    }
    if transcript.steps.is_empty() {
        return Err("peer transcript did not record any steps".into());
    }
    for step in &transcript.steps {
        match step.status.as_str() {
            "passed" | "unsupported" | "skipped" => {}
            other => {
                return Err(format!("step `{}` used unknown status `{other}`", step.name).into());
            }
        }
    }

    for capability in &transcript.capabilities {
        match capability.status.as_str() {
            "passed" | "unsupported" | "skipped" => {}
            other => {
                return Err(format!(
                    "capability `{}` used unknown status `{other}`",
                    capability.id
                )
                .into());
            }
        }
    }

    for capability_id in profile.required_passed {
        if transcript.capability_status(capability_id) != Some("passed") {
            return Err(format!(
                "capability `{capability_id}` expected `passed`, found {:?}",
                transcript.capability_status(capability_id)
            )
            .into());
        }
    }

    for capability_id in profile.allowed_unsupported {
        match transcript.capability_status(capability_id) {
            Some("passed" | "unsupported" | "skipped") => {}
            other => {
                return Err(format!(
                    "capability `{capability_id}` expected a recorded status, found {other:?}"
                )
                .into());
            }
        }
    }

    if profile.expects_group_round_trip
        && transcript.artifact_u8("round_trip_value") != Some(KNX_WRITE_VALUE)
    {
        return Err(format!(
            "round_trip_value expected {KNX_WRITE_VALUE}, found {:?}",
            transcript.artifact_u8("round_trip_value")
        )
        .into());
    }

    Ok(())
}

async fn run_peer_profile(profile: PeerProfile) -> TestResult {
    assert_policy("deterministic", "ignored")?;
    assert_peer_lane(profile.peer, profile.peer_lane)?;
    for profile_id in profile.profiles {
        assert_profile_lane(profile_id, "interop").or_else(|_| {
            if *profile_id == "secure_future" {
                assert_profile_lane(profile_id, "future")
            } else {
                assert_profile_lane(profile_id, "deterministic")
            }
        })?;
    }

    let lock = INTEROP_PORT_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock.lock().await;

    let table = standard_group_table()?;
    let bind_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, KNX_INTEROP_PORT));
    let mut config = KnxServerConfig::default();
    config.tunnel_behavior.ldata_con_enabled = false;
    let harness = ServerHarness::start_with_table_at(config, table.clone(), bind_addr).await?;
    let transcript_path = temp_transcript_path(profile.target);
    let sut_addr = harness.addr.to_string();

    let result: TestResult = async {
        let envs = vec![
            ("MABI_KNX_INTEROP_TARGET", profile.target.to_string()),
            ("MABI_KNX_SUT_HOST", "127.0.0.1".to_string()),
            ("MABI_KNX_SUT_PORT", KNX_INTEROP_PORT.to_string()),
            ("MABI_KNX_GROUP_ADDRESS", SCALING.to_string()),
            ("MABI_KNX_WRITE_VALUE", KNX_WRITE_VALUE.to_string()),
            (
                "MABI_KNX_TRANSCRIPT_PATH",
                transcript_path.display().to_string(),
            ),
        ];

        let output = run_peer_command(
            profile.program,
            &[profile.script.clone()],
            &envs,
            PEER_TIMEOUT,
        )
        .await?;
        assert_process_success(&profile, &transcript_path, &output)?;

        let transcript = load_peer_transcript(&transcript_path)?;
        assert_peer_transcript(&transcript, &profile, &sut_addr)?;

        if profile.expects_group_round_trip {
            let scaling: GroupAddress = SCALING.parse()?;
            let stored = table.read(&scaling)?;
            if stored != vec![KNX_WRITE_VALUE] {
                return Err(format!(
                    "expected server-side scaling value [{KNX_WRITE_VALUE}], found {stored:?}"
                )
                .into());
            }
        }

        Ok(())
    }
    .await;

    harness.shutdown().await?;
    let _ = fs::remove_file(&transcript_path);
    result
}

#[tokio::test]
#[ignore]
async fn xknx_canary_profile_smoke_contract() -> TestResult {
    run_peer_profile(PeerProfile {
        target: "xknx-canary",
        peer: "xknx",
        peer_lane: "canary_interop",
        profiles: &["xknx_canary"],
        program: "python3",
        script: peer_script("xknx", "peer_client.py"),
        required_passed: &[
            "discovery",
            "description",
            "tunneling_connect",
            "connection_state",
            "group_value_read_write",
        ],
        allowed_unsupported: &[],
        expects_group_round_trip: true,
    })
    .await
}

#[tokio::test]
#[ignore]
async fn calimero_tools_profile_smoke_contract() -> TestResult {
    run_peer_profile(PeerProfile {
        target: "calimero-tools",
        peer: "calimero_tools",
        peer_lane: "active_interop",
        profiles: &["group_io", "routing_busmonitor"],
        program: "python3",
        script: peer_script("calimero-tools", "peer_client.py"),
        required_passed: &[
            "discovery",
            "description",
            "tunneling_connect",
            "group_value_read_write",
            "busmonitor",
        ],
        allowed_unsupported: &[],
        expects_group_round_trip: true,
    })
    .await
}

#[tokio::test]
#[ignore]
async fn knxd_profile_smoke_contract() -> TestResult {
    run_peer_profile(PeerProfile {
        target: "knxd",
        peer: "knxd",
        peer_lane: "active_interop",
        profiles: &["tunnel_resilience", "routing_busmonitor"],
        program: "python3",
        script: peer_script("knxd", "peer_client.py"),
        required_passed: &[
            "tunneling_connect",
            "connection_state",
            "group_value_read_write",
            "sequence_ack_retry",
            "heartbeat_timeout",
        ],
        allowed_unsupported: &["routing_multicast"],
        expects_group_round_trip: true,
    })
    .await
}

#[tokio::test]
#[ignore]
async fn knxjs_profile_smoke_contract() -> TestResult {
    run_peer_profile(PeerProfile {
        target: "knxjs",
        peer: "knxjs",
        peer_lane: "active_interop",
        profiles: &["group_io", "dpt_matrix", "routing_busmonitor"],
        program: "node",
        script: peer_script("knxjs", "peer_client.js"),
        required_passed: &[
            "discovery",
            "tunneling_connect",
            "group_value_read_write",
            "dpt_codec",
        ],
        allowed_unsupported: &["routing_multicast"],
        expects_group_round_trip: true,
    })
    .await
}

#[tokio::test]
#[ignore]
async fn openknx_thelsing_profile_smoke_contract() -> TestResult {
    run_peer_profile(PeerProfile {
        target: "openknx-thelsing",
        peer: "openknx_thelsing",
        peer_lane: "corpus_optional_interop",
        profiles: &["secure_future", "group_io", "dpt_matrix"],
        program: "python3",
        script: peer_script("openknx-thelsing", "peer_client.py"),
        required_passed: &["group_value_read_write", "dpt_codec"],
        allowed_unsupported: &["secure_future"],
        expects_group_round_trip: true,
    })
    .await
}
