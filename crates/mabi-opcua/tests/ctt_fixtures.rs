use std::path::PathBuf;

use mabi_opcua::{
    compile_session_with_report, load_simulator_config, modeling::OpcUaCompiledLaunchConfig,
    TransportProtocol,
};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn https_fixture_reports_cache_and_transport_scheme() {
    let path = fixture_path("https_session.yaml");
    let config = load_simulator_config(&path).unwrap();

    let (compiled, report) =
        compile_session_with_report(&config, "https_demo", Some(&path)).unwrap();
    let launch: OpcUaCompiledLaunchConfig =
        serde_json::from_value(compiled.launch.config.clone()).unwrap();
    assert_eq!(
        launch.server_config.endpoint_protocol,
        TransportProtocol::Https
    );
    assert!(!compiled.catalog.nodes.is_empty());
    assert!(report.cache_dir.contains("mabi-opcua"));
}
