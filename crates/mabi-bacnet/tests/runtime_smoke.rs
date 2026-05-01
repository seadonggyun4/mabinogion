mod support;

use mabi_core::Protocol;

use support::runtime_harness::{start_runtime_session, start_runtime_session_with_objects};

#[tokio::test]
async fn runtime_smoke_uses_the_canonical_bacnet_driver_path() {
    let session = start_runtime_session(4501).await;
    let device_ids = session.devices().device_ids();
    assert_eq!(device_ids, vec!["bacnet-4501".to_string()]);

    let snapshots = session
        .snapshots()
        .await
        .expect("runtime session should expose snapshots");
    assert_eq!(snapshots.len(), 1);
    let snapshot = &snapshots[0];
    assert_eq!(snapshot.protocol, Some(Protocol::BacnetIp));
    assert_eq!(
        snapshot
            .metadata
            .get("device_instance")
            .and_then(|value| value.as_u64()),
        Some(4501)
    );
    assert!(snapshot.metadata.contains_key("bind_address"));
    assert!(snapshot.metadata.contains_key("metrics"));

    session
        .stop()
        .await
        .expect("runtime session should stop cleanly");
}

#[tokio::test]
async fn runtime_zero_demo_objects_keeps_empty_user_registry() {
    let session = start_runtime_session_with_objects(4502, 0).await;
    let device = session
        .devices()
        .get("bacnet-4502")
        .expect("runtime should register the BACnet device port");

    let read_result = device.read("0:0").await;
    assert!(
        read_result.is_err(),
        "objects=0 should not create demo Analog Input objects"
    );

    let snapshots = session
        .snapshots()
        .await
        .expect("runtime session should expose snapshots");
    assert_eq!(
        snapshots[0]
            .metadata
            .get("objects")
            .and_then(|value| value.as_u64()),
        Some(0)
    );

    session
        .stop()
        .await
        .expect("runtime session should stop cleanly");
}
