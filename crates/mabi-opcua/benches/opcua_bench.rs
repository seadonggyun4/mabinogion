//! Benchmarks for OPC UA simulator.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::sync::Arc;

use mabi_opcua::{
    nodes::base::{LocalizedText, NodeClass, QualifiedName},
    nodes::cache::CachedNode,
    nodes::{AddressSpace, AddressSpaceConfig, NodeCache, NodeCacheConfig},
    services::{
        HistoryStore, HistoryStoreConfig, SubscriptionConfig, SubscriptionManager,
        SubscriptionManagerConfig,
    },
    types::{variant::DataTypeId, DataValue, NodeId, Variant},
};

fn bench_address_space_add_nodes(c: &mut Criterion) {
    let mut group = c.benchmark_group("address_space_add");
    let objects_folder = NodeId::numeric(0, 85); // Objects folder

    for count in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::new("nodes", count), count, |b, &count| {
            b.iter(|| {
                let config = AddressSpaceConfig::default();
                let address_space = AddressSpace::new(config);

                for i in 0..count {
                    let _ = address_space.add_variable(
                        NodeId::numeric(2, i as u32),
                        QualifiedName::new(2, format!("Variable{}", i)),
                        format!("Variable{}", i),
                        NodeId::numeric(0, DataTypeId::Double as u32),
                        Variant::Double(i as f64),
                        &objects_folder,
                    );
                }

                black_box(address_space.node_count())
            });
        });
    }

    group.finish();
}

fn bench_address_space_read(c: &mut Criterion) {
    // Setup: Create address space with nodes
    let config = AddressSpaceConfig::default();
    let address_space = AddressSpace::new(config);
    let objects_folder = NodeId::numeric(0, 85); // Objects folder

    for i in 0..10000 {
        let _ = address_space.add_variable(
            NodeId::numeric(2, i as u32),
            QualifiedName::new(2, format!("Variable{}", i)),
            format!("Variable{}", i),
            NodeId::numeric(0, DataTypeId::Double as u32),
            Variant::Double(i as f64),
            &objects_folder,
        );
    }

    let address_space = Arc::new(address_space);

    c.bench_function("address_space_read_single", |b| {
        b.iter(|| {
            let node_id = NodeId::numeric(2, 5000);
            black_box(address_space.read_value(&node_id))
        });
    });
}

fn bench_node_cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("node_cache");

    let config = NodeCacheConfig {
        max_size: 1000,
        ..Default::default()
    };

    let cache = NodeCache::new(config);

    // Pre-populate
    for i in 0..500u32 {
        let node_id = NodeId::numeric(2, i);
        cache.put(CachedNode {
            node_id: node_id.clone(),
            node_class: NodeClass::Variable,
            browse_name: QualifiedName::new(2, format!("Var{}", i)),
            display_name: LocalizedText::invariant(format!("Var{}", i)),
            value: None,
            references: None,
        });
    }

    group.bench_function("cache_hit", |b| {
        b.iter(|| {
            let node_id = NodeId::numeric(2, 250);
            black_box(cache.get(&node_id))
        });
    });

    group.bench_function("cache_miss_and_insert", |b| {
        let mut counter = 1000u32;
        b.iter(|| {
            let node_id = NodeId::numeric(2, counter);
            counter += 1;
            cache.put(CachedNode {
                node_id: node_id.clone(),
                node_class: NodeClass::Variable,
                browse_name: QualifiedName::new(2, format!("Var{}", counter)),
                display_name: LocalizedText::invariant(format!("Var{}", counter)),
                value: None,
                references: None,
            });
        });
    });

    group.finish();
}

fn bench_subscription_manager(c: &mut Criterion) {
    let mut group = c.benchmark_group("subscription");

    let config = SubscriptionManagerConfig {
        max_subscriptions: 10000,
        ..Default::default()
    };
    let manager = SubscriptionManager::with_config(config);

    // Create subscriptions
    for _ in 0..1000 {
        manager.create(SubscriptionConfig::default());
    }

    group.bench_function("create_subscription", |b| {
        b.iter(|| black_box(manager.create(SubscriptionConfig::default())));
    });

    group.bench_function("get_subscription", |b| {
        b.iter(|| black_box(manager.get(500)));
    });

    group.finish();
}

fn bench_history_store(c: &mut Criterion) {
    let mut group = c.benchmark_group("history");

    let config = HistoryStoreConfig::default().with_max_values(100000);
    let store = HistoryStore::new(config);
    let node_id = NodeId::numeric(2, 1001);

    // Record some history
    for _ in 0..10000 {
        store.record_value(&node_id, DataValue::new(Variant::Double(25.0)));
    }

    group.bench_function("record_value", |b| {
        b.iter(|| {
            store.record_value(
                &NodeId::numeric(2, 1001),
                DataValue::new(Variant::Double(25.0)),
            );
        });
    });

    group.finish();
}

fn bench_node_id_parsing(c: &mut Criterion) {
    c.bench_function("node_id_parse_numeric", |b| {
        b.iter(|| black_box("ns=2;i=1001".parse::<NodeId>()));
    });

    c.bench_function("node_id_parse_string", |b| {
        b.iter(|| black_box("ns=2;s=Temperature".parse::<NodeId>()));
    });
}

criterion_group!(
    benches,
    bench_address_space_add_nodes,
    bench_address_space_read,
    bench_node_cache,
    bench_subscription_manager,
    bench_history_store,
    bench_node_id_parsing,
);

criterion_main!(benches);
