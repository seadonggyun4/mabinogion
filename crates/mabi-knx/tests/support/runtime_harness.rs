use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use mabi_runtime::{
    ProtocolDriverRegistry, ProtocolLaunchSpec, RuntimeExtensions, RuntimeSession,
    RuntimeSessionSpec,
};
use serde_json::json;
use tokio::time::Duration;

use super::TestResult;

pub async fn launch_knx_runtime_session() -> TestResult<RuntimeSession> {
    let mut registry = ProtocolDriverRegistry::new();
    registry.register(mabi_knx::runtime::driver());

    let spec = RuntimeSessionSpec {
        services: vec![ProtocolLaunchSpec {
            protocol: "knx".to_string(),
            name: Some("knx-phase1-smoke".to_string()),
            config: json!({
                "bind_addr": SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)),
                "individual_address": "1.1.1",
                "group_objects": 8
            }),
        }],
        readiness_timeout: Some(2_000),
    };

    let session = RuntimeSession::new(spec, &registry, RuntimeExtensions::default()).await?;
    session.start(Duration::from_secs(2)).await?;
    Ok(session)
}
