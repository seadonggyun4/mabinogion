mod support;

use mabi_core::Protocol;

use support::runtime_harness::start_runtime_session;

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
