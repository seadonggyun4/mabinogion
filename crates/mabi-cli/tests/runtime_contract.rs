use std::net::{SocketAddr, TcpListener};

use mabi_cli::runtime_registry::workspace_protocol_registry;
use mabi_runtime::{
    ProtocolLaunchSpec, RuntimeErrorKind, RuntimeExtensions, RuntimeSession, RuntimeSessionSpec,
    RUNTIME_CONTRACT_VERSION, RUNTIME_METADATA_KEY,
};
use serde_json::json;
use tokio::time::Duration;

fn reserve_loopback_tcp_addr() -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("reserve loopback address");
    let address = listener.local_addr().expect("reserved address");
    drop(listener);
    address
}

fn launch_spec(protocol: &str) -> ProtocolLaunchSpec {
    let config = match protocol {
        "modbus" => json!({
            "transport": {
                "kind": "tcp",
                "bind_addr": reserve_loopback_tcp_addr().to_string(),
                "performance_preset": "default",
            },
            "devices": 1,
            "points_per_device": 4,
        }),
        "opcua" => json!({
            "bind_addr": "127.0.0.1:0",
            "endpoint_path": "/mabi/runtime-contract",
            "nodes": 4,
            "security_mode": "None",
        }),
        "bacnet" => json!({
            "bind_addr": "127.0.0.1:0",
            "device_instance": 9_101,
            "objects": 8,
            "bbmd_enabled": false,
        }),
        "knx" => json!({
            "bind_addr": "127.0.0.1:0",
            "individual_address": "1.1.1",
            "group_objects": 8,
        }),
        _ => panic!("unknown protocol {protocol}"),
    };

    ProtocolLaunchSpec {
        protocol: protocol.to_string(),
        name: Some(format!("contract-{protocol}")),
        config,
    }
}

async fn start_session(protocol: &str) -> RuntimeSession {
    let registry = workspace_protocol_registry();
    let spec = RuntimeSessionSpec {
        services: vec![launch_spec(protocol)],
        readiness_timeout: Some(1_000),
    };
    let session = RuntimeSession::new(spec, &registry, RuntimeExtensions::default())
        .await
        .expect("session builds");
    session
        .start(Duration::from_secs(1))
        .await
        .expect("session starts");
    session
}

fn assert_stable_metadata(protocol: &str, keys: &[String]) {
    let has = |key: &str| keys.iter().any(|entry| entry == key);
    match protocol {
        "modbus" => {
            for key in ["transport", "devices", "points", "metrics"] {
                assert!(has(key), "{protocol} should include stable metadata {key}");
            }
            assert!(
                has("bind_address") || has("rtu_transport"),
                "modbus should include a transport-specific stable address key"
            );
        }
        "opcua" => {
            for key in [
                "endpoint",
                "transport_protocol",
                "nodes",
                "devices",
                "namespaces",
                "security_profile",
                "durability_mode",
                "restored_subscriptions",
                "detached_restored_subscriptions",
                "generated_types",
                "stats",
            ] {
                assert!(has(key), "{protocol} should include stable metadata {key}");
            }
        }
        "bacnet" => {
            for key in [
                "bind_address",
                "device_instance",
                "objects",
                "bbmd_enabled",
                "metrics",
            ] {
                assert!(has(key), "{protocol} should include stable metadata {key}");
            }
        }
        "knx" => {
            for key in [
                "bind_address",
                "individual_address",
                "group_objects",
                "metrics",
            ] {
                assert!(has(key), "{protocol} should include stable metadata {key}");
            }
        }
        _ => panic!("unknown protocol {protocol}"),
    }
}

#[tokio::test]
async fn runtime_contract_snapshots_include_runtime_metadata() {
    for protocol in ["modbus", "opcua", "bacnet", "knx"] {
        let session = start_session(protocol).await;
        let snapshots = session.snapshots().await.expect("snapshots");
        assert_eq!(snapshots.len(), 1);

        let snapshot = &snapshots[0];
        assert!(snapshot.metadata.contains_key(RUNTIME_METADATA_KEY));
        let runtime = snapshot.runtime_metadata().expect("runtime metadata");
        assert_eq!(runtime.contract_version, RUNTIME_CONTRACT_VERSION);
        assert_eq!(runtime.service_name, format!("contract-{protocol}"));
        assert!(runtime.ready);

        let keys = snapshot.metadata.keys().cloned().collect::<Vec<_>>();
        assert_stable_metadata(protocol, &keys);

        let session_snapshot = session.session_snapshot().await.expect("session snapshot");
        assert_eq!(session_snapshot.contract_version, RUNTIME_CONTRACT_VERSION);
        assert_eq!(session_snapshot.services.len(), 1);

        session.stop().await.expect("session stops");
    }
}

#[tokio::test]
async fn invalid_launch_config_returns_config_error() {
    let registry = workspace_protocol_registry();
    let spec = RuntimeSessionSpec {
        services: vec![ProtocolLaunchSpec {
            protocol: "modbus".to_string(),
            name: Some("bad-modbus".to_string()),
            config: json!({
                "transport": {
                    "kind": "tcp",
                    "bind_addr": "not-an-address",
                    "performance_preset": "default",
                }
            }),
        }],
        readiness_timeout: Some(1_000),
    };

    let error = match RuntimeSession::new(spec, &registry, RuntimeExtensions::default()).await {
        Ok(_) => panic!("invalid config should fail"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), RuntimeErrorKind::ConfigError);
    assert_eq!(error.info().kind, RuntimeErrorKind::ConfigError);
}
