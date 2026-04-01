#[test]
#[ignore]
fn transport_perf_contracts_are_release_only() {
    assert!(
        !cfg!(debug_assertions),
        "run ignored transport perf contracts with --release"
    );
}
