mod support;

use support::contract::contract;

#[test]
fn perf_lane_policy_is_release_only_ignored() {
    let verification = contract();

    assert!(verification.baseline.perf_contract_present);
    assert_eq!(verification.policies.default_workspace_lane, "deterministic");
    assert_eq!(verification.policies.perf_lane, "release_only_ignored");
    assert!(
        verification.policies.default_perf_thresholds_forbidden,
        "threshold-based perf assertions must stay out of the default workspace lane",
    );
}

#[test]
fn deterministic_profiles_do_not_redeclare_perf_lanes() {
    let verification = contract();

    for profile in &verification.profiles {
        assert_eq!(
            profile.lane, "deterministic",
            "Phase 1-4 BACnet profiles should stay in the deterministic lane; \
             perf belongs to the dedicated release-only ignored policy lane"
        );
    }
}

#[test]
#[ignore]
fn bacnet_perf_contracts_are_release_only() {
    assert!(
        !cfg!(debug_assertions),
        "run ignored BACnet perf contracts with --release"
    );
}
