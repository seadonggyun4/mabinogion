use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use mabi_opcua::{
    compile_session, load_simulator_config, modeling::OpcUaCompiledLaunchConfig, OpcUaServer,
    OpcUaServerConfig, OpcUaSimulatorConfig, PresetDefinition, SessionControlConfig,
    SessionDefinition, TransportConnectionMode, TransportProtocol,
};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn https_fixture_compiles_to_protocol_aware_endpoint() {
    let path = fixture_path("https_session.yaml");
    let config = load_simulator_config(&path).unwrap();
    let summary = config.inspect_summary();
    let session = summary
        .sessions
        .iter()
        .find(|session| session.name == "https_demo")
        .unwrap();
    assert_eq!(session.transport_protocol, TransportProtocol::Https);

    let compiled = compile_session(&config, "https_demo", Some(&path)).unwrap();
    let launch: OpcUaCompiledLaunchConfig =
        serde_json::from_value(compiled.launch.config.clone()).unwrap();
    assert_eq!(
        launch.server_config.endpoint_protocol,
        TransportProtocol::Https
    );
    assert_eq!(
        launch.server_config.endpoint_url,
        "https://127.0.0.1:4843/ua"
    );
    assert!(launch
        .server_config
        .certificate_path
        .as_ref()
        .unwrap()
        .ends_with("server-cert.pem"));
}

#[test]
fn reverse_connect_fixture_compiles_to_protocol_aware_endpoint() {
    let path = fixture_path("reverse_connect_session.yaml");
    let config = load_simulator_config(&path).unwrap();
    let summary = config.inspect_summary();
    let session = summary
        .sessions
        .iter()
        .find(|session| session.name == "reverse_demo")
        .unwrap();
    assert_eq!(session.transport_protocol, TransportProtocol::OpcTcp);
    assert_eq!(
        session.transport_connection_mode,
        TransportConnectionMode::ReverseConnect
    );

    let compiled = compile_session(&config, "reverse_demo", Some(&path)).unwrap();
    let launch: OpcUaCompiledLaunchConfig =
        serde_json::from_value(compiled.launch.config.clone()).unwrap();
    assert_eq!(
        launch.server_config.connection_mode,
        TransportConnectionMode::ReverseConnect
    );
    assert_eq!(
        launch.server_config.reverse_connect_target.as_deref(),
        Some("opc.tcp://127.0.0.1:4940")
    );
    assert_eq!(
        launch.server_config.endpoint_url,
        "opc.tcp://127.0.0.1:4840/ua"
    );
}

#[test]
fn https_reverse_connect_is_rejected_by_validation() {
    let config = OpcUaSimulatorConfig {
        transports: BTreeMap::from([(
            "bad".into(),
            mabi_opcua::modeling::TransportDefinition {
                protocol: TransportProtocol::Https,
                connection_mode: TransportConnectionMode::ReverseConnect,
                bind: "127.0.0.1".into(),
                port: 4843,
                endpoint_path: "/ua".into(),
                reverse_connect_target: Some("opc.tcp://127.0.0.1:4940".into()),
                retry_interval_ms: 250,
                security_profile: Some("none".into()),
                server_name: None,
                certificate_path: Some("server-cert.pem".into()),
                private_key_path: Some("server-key.pem".into()),
            },
        )]),
        security_profiles: BTreeMap::from([(
            "none".into(),
            mabi_opcua::SecurityProfileDefinition::default(),
        )]),
        sessions: BTreeMap::from([(
            "demo".into(),
            SessionDefinition {
                transport: "bad".into(),
                models: Vec::new(),
                devices: Vec::new(),
                preset: Some("generated".into()),
                service_name: Some("demo".into()),
                readiness_timeout_ms: Some(5_000),
                control: SessionControlConfig::default(),
                runtime: mabi_opcua::SessionRuntimeConfig::default(),
            },
        )]),
        presets: BTreeMap::from([("generated".into(), PresetDefinition::default())]),
        ..Default::default()
    };

    let error = config.validate(None).unwrap_err();
    assert!(error
        .to_string()
        .contains("only supports reverse_connect with opc.tcp"));
}

#[tokio::test]
async fn https_server_requires_certificate_paths_on_start() {
    let server = OpcUaServer::new(OpcUaServerConfig {
        endpoint_url: "https://127.0.0.1:0/ua".to_string(),
        endpoint_protocol: TransportProtocol::Https,
        ..Default::default()
    })
    .unwrap();

    let error = server.start().await.unwrap_err();
    assert!(error.to_string().contains("certificate_path"));
}

#[tokio::test]
async fn reverse_connect_establishes_outbound_tcp_connection_and_retries() {
    let acceptor = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = acceptor.local_addr().unwrap();

    let server = OpcUaServer::new(OpcUaServerConfig {
        endpoint_url: "opc.tcp://127.0.0.1:0/ua".to_string(),
        endpoint_protocol: TransportProtocol::OpcTcp,
        connection_mode: TransportConnectionMode::ReverseConnect,
        reverse_connect_target: Some(format!("opc.tcp://{}", target)),
        retry_interval_ms: 50,
        ..Default::default()
    })
    .unwrap();

    server.start().await.unwrap();

    let (stream, _) = tokio::time::timeout(Duration::from_secs(2), acceptor.accept())
        .await
        .unwrap()
        .unwrap();
    drop(stream);

    let (stream, _) = tokio::time::timeout(Duration::from_secs(2), acceptor.accept())
        .await
        .unwrap()
        .unwrap();
    drop(stream);

    server.stop().await.unwrap();
}
