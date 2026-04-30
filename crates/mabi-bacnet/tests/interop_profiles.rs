mod support;

use std::fs;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::Path;
use std::time::Duration;

use mabi_bacnet::object::BACnetObject;
use mabi_bacnet::prelude::BACnetServer;

use support::assertions::{
    assert_active_peer_transcript, assert_bacpypes3_canary_transcript,
    assert_capability_interop_coverage, assert_peer_ci_participation, assert_peer_lane,
    ActivePeerExpectations,
};
use support::contract::contract;
use support::fixtures::{loopback_server_config, property_fixture};
use support::interop::{
    bac0_peer_script, bacnet4j_peer_script, bacnet_stack_peer_script, bacpypes3_peer_script,
    load_active_peer_transcript, load_bacpypes3_transcript, run_python_peer, run_shell_peer,
    temp_transcript_path,
};
use support::server_harness::BacnetServerHarness;

const INTEROP_SERVER_PORT: u16 = 47_808;
const INTEROP_WRITE_VALUE: f64 = 42.5;
const INTEROP_TIMEOUT: Duration = Duration::from_secs(75);

fn interop_server_config(device_instance: u32) -> mabi_bacnet::prelude::ServerConfig {
    let mut config = loopback_server_config(device_instance);
    config.bind_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, INTEROP_SERVER_PORT));
    config.broadcast_addr =
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, INTEROP_SERVER_PORT));
    config
}

fn property_fixture_envs(
    sut_addr: &str,
    device_instance: u32,
    object_instance: u32,
    transcript_path: &Path,
) -> Vec<(&'static str, String)> {
    vec![
        ("MABI_BACNET_SUT_ADDR", sut_addr.to_string()),
        ("MABI_BACNET_DEVICE_INSTANCE", device_instance.to_string()),
        (
            "MABI_BACNET_OBJECT_ID",
            format!("analog-output,{object_instance}"),
        ),
        (
            "MABI_BACNET_OBJECT_TYPE_HYPHEN",
            "analog-output".to_string(),
        ),
        ("MABI_BACNET_OBJECT_TYPE_CAMEL", "analogOutput".to_string()),
        ("MABI_BACNET_OBJECT_INSTANCE", object_instance.to_string()),
        ("MABI_BACNET_PROPERTY_ID", "present-value".to_string()),
        ("MABI_BACNET_PROPERTY_ID_CAMEL", "presentValue".to_string()),
        (
            "MABI_BACNET_RPM_PROPERTIES_HYPHEN",
            "present-value,object-name".to_string(),
        ),
        (
            "MABI_BACNET_RPM_PROPERTIES_CAMEL",
            "presentValue,objectName".to_string(),
        ),
        (
            "MABI_BACNET_EXPECTED_OBJECT_NAME",
            format!("AO_{object_instance}"),
        ),
        ("MABI_BACNET_WRITE_VALUE", INTEROP_WRITE_VALUE.to_string()),
        (
            "MABI_BACNET_TRANSCRIPT_PATH",
            transcript_path.display().to_string(),
        ),
    ]
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

fn assert_process_success(peer_name: &str, transcript_path: &Path, output: &std::process::Output) {
    if output.status.success() {
        return;
    }

    panic!(
        "{peer_name} interop peer exited with status {}\nstdout:\n{}\nstderr:\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        transcript_debug_hint(transcript_path),
    );
}

fn assert_interop_policies() {
    let verification = contract();
    assert_eq!(verification.policies.interop_lane, "ignored");
    assert_eq!(
        verification.policies.default_workspace_lane,
        "deterministic"
    );
}

#[tokio::test]
#[ignore]
async fn bacpypes3_canary_profile_smoke_contract() {
    assert_interop_policies();
    assert_peer_lane("bacpypes3", "active_interop");
    assert_peer_ci_participation("bacpypes3", false);
    assert_capability_interop_coverage("discovery", "active");
    assert_capability_interop_coverage("property_io", "active");

    let fixture = property_fixture();
    let analog_output_id = fixture.analog_output.object_identifier();
    let device_instance = 4_201;
    let server = BACnetServer::new(interop_server_config(device_instance), fixture.registry);
    let harness = BacnetServerHarness::start(server).await;
    let transcript_path = temp_transcript_path("bacpypes3-canary");
    let sut_addr = harness.addr().to_string();

    let result: Result<(), String> = async {
        let envs = property_fixture_envs(
            &sut_addr,
            device_instance,
            analog_output_id.instance,
            &transcript_path,
        );

        let output =
            run_python_peer(&bacpypes3_peer_script(), &envs, Duration::from_secs(45)).await;
        assert_process_success("BACpypes3", &transcript_path, &output);

        let transcript = load_bacpypes3_transcript(&transcript_path);
        assert_bacpypes3_canary_transcript(
            &transcript,
            &sut_addr,
            device_instance,
            INTEROP_WRITE_VALUE,
        );
        Ok(())
    }
    .await;

    harness.shutdown().await;
    let _ = fs::remove_file(&transcript_path);
    result.unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
#[ignore]
async fn bac0_canary_profile_smoke_contract() {
    assert_interop_policies();
    assert_peer_lane("bac0", "active_interop");
    assert_peer_ci_participation("bac0", false);
    assert_capability_interop_coverage("property_io", "active");
    assert_capability_interop_coverage("property_multiple", "active");

    let fixture = property_fixture();
    let analog_output_id = fixture.analog_output.object_identifier();
    let device_instance = 4_202;
    let server = BACnetServer::new(interop_server_config(device_instance), fixture.registry);
    let harness = BacnetServerHarness::start(server).await;
    let transcript_path = temp_transcript_path("bac0-canary");
    let sut_addr = harness.addr().to_string();

    let result: Result<(), String> = async {
        let envs = property_fixture_envs(
            &sut_addr,
            device_instance,
            analog_output_id.instance,
            &transcript_path,
        );

        let output = run_python_peer(&bac0_peer_script(), &envs, INTEROP_TIMEOUT).await;
        assert_process_success("BAC0", &transcript_path, &output);

        let transcript = load_active_peer_transcript(&transcript_path);
        assert_active_peer_transcript(
            &transcript,
            &sut_addr,
            device_instance,
            ActivePeerExpectations {
                peer: "bac0",
                require_discovery: false,
                require_read: true,
                require_write: true,
                require_property_multiple: true,
                expected_round_trip_value: Some(INTEROP_WRITE_VALUE),
            },
        );
        Ok(())
    }
    .await;

    harness.shutdown().await;
    let _ = fs::remove_file(&transcript_path);
    result.unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
#[ignore]
async fn bacnet_stack_canary_profile_smoke_contract() {
    assert_interop_policies();
    assert_peer_lane("bacnet-stack", "active_interop");
    assert_peer_ci_participation("bacnet-stack", false);
    assert_capability_interop_coverage("discovery", "active");

    let fixture = property_fixture();
    let analog_output_id = fixture.analog_output.object_identifier();
    let device_instance = 4_203;
    let server = BACnetServer::new(interop_server_config(device_instance), fixture.registry);
    let harness = BacnetServerHarness::start(server).await;
    let transcript_path = temp_transcript_path("bacnet-stack-canary");
    let sut_addr = harness.addr().to_string();

    let result: Result<(), String> = async {
        let envs = property_fixture_envs(
            &sut_addr,
            device_instance,
            analog_output_id.instance,
            &transcript_path,
        );

        let output =
            run_shell_peer(&bacnet_stack_peer_script(), &envs, Duration::from_secs(90)).await;
        assert_process_success("bacnet-stack", &transcript_path, &output);

        let transcript = load_active_peer_transcript(&transcript_path);
        assert_active_peer_transcript(
            &transcript,
            &sut_addr,
            device_instance,
            ActivePeerExpectations {
                peer: "bacnet-stack",
                require_discovery: true,
                require_read: false,
                require_write: false,
                require_property_multiple: false,
                expected_round_trip_value: None,
            },
        );
        Ok(())
    }
    .await;

    harness.shutdown().await;
    let _ = fs::remove_file(&transcript_path);
    result.unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
#[ignore]
async fn bacnet4j_canary_profile_smoke_contract() {
    assert_interop_policies();
    assert_peer_lane("bacnet4j", "active_interop");
    assert_peer_ci_participation("bacnet4j", false);
    assert_capability_interop_coverage("property_io", "active");
    assert_capability_interop_coverage("property_multiple", "active");

    let fixture = property_fixture();
    let analog_output_id = fixture.analog_output.object_identifier();
    let device_instance = 4_204;
    let server = BACnetServer::new(interop_server_config(device_instance), fixture.registry);
    let harness = BacnetServerHarness::start(server).await;
    let transcript_path = temp_transcript_path("bacnet4j-canary");
    let sut_addr = harness.addr().to_string();

    let result: Result<(), String> = async {
        let envs = property_fixture_envs(
            &sut_addr,
            device_instance,
            analog_output_id.instance,
            &transcript_path,
        );

        let output = run_shell_peer(&bacnet4j_peer_script(), &envs, Duration::from_secs(120)).await;
        assert_process_success("BACnet4J", &transcript_path, &output);

        let transcript = load_active_peer_transcript(&transcript_path);
        assert_active_peer_transcript(
            &transcript,
            &sut_addr,
            device_instance,
            ActivePeerExpectations {
                peer: "bacnet4j",
                require_discovery: false,
                require_read: true,
                require_write: true,
                require_property_multiple: true,
                expected_round_trip_value: Some(INTEROP_WRITE_VALUE),
            },
        );
        Ok(())
    }
    .await;

    harness.shutdown().await;
    let _ = fs::remove_file(&transcript_path);
    result.unwrap_or_else(|error| panic!("{error}"));
}
