use std::path::PathBuf;

use mabi_opcua::{
    compile_session, load_simulator_config, BrowseDirection, NodeId, OpcUaServer,
    OpcUaServerConfig, TransportProtocol,
};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[tokio::test]
#[ignore = "containerized self-contained interop profile"]
async fn open62541_profile_smoke_contract() {
    let server = OpcUaServer::new(OpcUaServerConfig {
        endpoint_url: "opc.tcp://127.0.0.1:0".to_string(),
        ..Default::default()
    })
    .unwrap();

    let folder = server
        .add_folder("ns=2;s=Interop.Open62541", "Interop.Open62541", None)
        .unwrap();
    let value = server
        .add_writable_variable("ns=2;s=Interop.Open62541.Value", "Value", 21.5f64)
        .unwrap();

    server.start().await.unwrap();

    let browse = server.address_space().browse(
        &NodeId::objects_folder(),
        BrowseDirection::Forward,
        None,
        false,
        None,
        100,
    );
    assert!(browse
        .references
        .iter()
        .any(|reference| reference.node_id == folder));
    assert_eq!(
        server
            .read_value(&value)
            .value()
            .and_then(|value| value.as_f64()),
        Some(21.5)
    );

    server.stop().await.unwrap();
}

#[tokio::test]
#[ignore = "containerized self-contained interop profile"]
async fn milo_profile_smoke_contract() {
    let cert = fixture_path("server-cert.pem");
    let key = fixture_path("server-key.pem");
    let server = OpcUaServer::new(OpcUaServerConfig {
        endpoint_url: "https://127.0.0.1:0/ua".to_string(),
        endpoint_protocol: TransportProtocol::Https,
        certificate_path: Some(cert),
        private_key_path: Some(key),
        ..Default::default()
    })
    .unwrap();

    server
        .add_writable_variable("ns=2;s=Interop.Milo.Value", "Value", "ready")
        .unwrap();

    server.start().await.unwrap();
    let value = server.read_value(&NodeId::string(2, "Interop.Milo.Value"));
    assert_eq!(
        value.value().and_then(|value| value.as_str()),
        Some("ready")
    );
    server.stop().await.unwrap();
}

#[tokio::test]
#[ignore = "containerized self-contained interop profile"]
async fn async_opcua_profile_smoke_contract() {
    let path = fixture_path("https_session.yaml");
    let config = load_simulator_config(&path).unwrap();
    let compiled = compile_session(&config, "https_demo", Some(&path)).unwrap();

    let mut server_config: OpcUaServerConfig =
        serde_json::from_value(compiled.launch.config.clone()).unwrap();
    server_config.endpoint_protocol = TransportProtocol::OpcTcp;
    server_config.endpoint_url = "opc.tcp://127.0.0.1:0".to_string();

    let server = OpcUaServer::from_generated_catalog(server_config, &compiled.catalog).unwrap();

    assert!(!compiled.catalog.namespace_summary().is_empty());

    server.start().await.unwrap();
    assert!(server.address_space().node_count() > 0);
    server.stop().await.unwrap();
}
