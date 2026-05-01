use std::time::Duration;

use serde_json::json;

use mabi_runtime::{
    ProtocolDriverRegistry, ProtocolLaunchSpec, RuntimeExtensions, RuntimeSession,
    RuntimeSessionSpec,
};

use mabi_bacnet::runtime;

pub async fn start_runtime_session(device_instance: u32) -> RuntimeSession {
    start_runtime_session_with_objects(device_instance, 8).await
}

pub async fn start_runtime_session_with_objects(
    device_instance: u32,
    objects: usize,
) -> RuntimeSession {
    let mut registry = ProtocolDriverRegistry::new();
    registry.register(runtime::driver());

    let spec = RuntimeSessionSpec {
        services: vec![ProtocolLaunchSpec {
            protocol: "bacnet".into(),
            name: Some(format!("bacnet-runtime-{device_instance}")),
            config: json!({
                "bind_addr": "127.0.0.1:0",
                "device_instance": device_instance,
                "objects": objects,
                "bbmd_enabled": false
            }),
        }],
        readiness_timeout: Some(2_000),
    };

    let session = RuntimeSession::new(spec, &registry, RuntimeExtensions::default())
        .await
        .expect("runtime session should build");
    session
        .start(Duration::from_secs(2))
        .await
        .expect("runtime session should start");
    session
}
