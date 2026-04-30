mod support;

use serde_json::Value;

use support::contract::assert_profile_lane;
use support::runtime_harness::launch_knx_runtime_session;
use support::TestResult;

#[tokio::test]
async fn runtime_smoke_profile() -> TestResult {
    assert_profile_lane("runtime_smoke", "deterministic")?;

    let session = launch_knx_runtime_session().await?;
    let devices = session.devices();
    assert!(devices.get("knx-1-1-1").is_some());

    let snapshots = session.snapshots().await?;
    let snapshot = snapshots
        .iter()
        .find(|snapshot| snapshot.name == "knx-phase1-smoke")
        .ok_or("missing KNX runtime snapshot")?;
    assert!(snapshot.status.ready);
    assert_eq!(
        snapshot
            .metadata
            .get("individual_address")
            .and_then(Value::as_str),
        Some("1.1.1")
    );
    assert_eq!(
        snapshot
            .metadata
            .get("group_objects")
            .and_then(Value::as_u64),
        Some(8)
    );
    assert!(snapshot.metadata.contains_key("metrics"));

    session.stop().await?;
    Ok(())
}
