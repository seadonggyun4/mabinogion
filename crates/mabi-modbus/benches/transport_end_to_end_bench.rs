#[path = "../tests/support/transport_harness.rs"]
mod transport_harness;

use std::time::Instant;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use mabi_modbus::rtu::PerformancePreset as RtuPerformancePreset;
use mabi_modbus::tcp::PerformancePreset as TcpPerformancePreset;
use transport_harness::{
    RtuChannelHarness, RtuTcpBridgeHarness, TcpRoundTripClient, TcpServerHarness,
};

fn benchmark_tcp_end_to_end(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(4)
        .build()
        .expect("benchmark runtime");
    let mut group = c.benchmark_group("tcp_end_to_end");
    group.throughput(Throughput::Elements(1));

    let mut harnesses = Vec::new();
    let multi_units = vec![1, 2, 3, 4, 5, 6, 7, 8];

    for (label, preset) in [
        ("default", TcpPerformancePreset::Default),
        ("high_throughput", TcpPerformancePreset::HighThroughput),
    ] {
        let single_harness = runtime.block_on(TcpServerHarness::start(preset, &[1]));
        let single_addr = single_harness.addr();
        harnesses.push(single_harness);

        group.bench_function(BenchmarkId::new("single_client_fc03", label), |b| {
            b.iter_custom(|iters| {
                runtime.block_on(async {
                    let mut client = TcpRoundTripClient::connect(single_addr).await;
                    let _ = client.read_holding_register(1, 0).await.unwrap();

                    let started = Instant::now();
                    for _ in 0..iters {
                        black_box(client.read_holding_register(1, 0).await.unwrap());
                    }
                    started.elapsed()
                })
            });
        });

        let multi_harness = runtime.block_on(TcpServerHarness::start(preset, &multi_units));
        let multi_addr = multi_harness.addr();
        harnesses.push(multi_harness);

        group.bench_function(BenchmarkId::new("multi_unit_round_robin", label), |b| {
            let unit_ids = multi_units.clone();
            b.iter_custom(|iters| {
                runtime.block_on(async {
                    let mut client = TcpRoundTripClient::connect(multi_addr).await;
                    let _ = client.read_holding_register(unit_ids[0], 0).await.unwrap();

                    let started = Instant::now();
                    for step in 0..iters {
                        let unit_id = unit_ids[step as usize % unit_ids.len()];
                        let address = (step as u16) & 0x001f;
                        black_box(
                            client
                                .read_holding_register(unit_id, address)
                                .await
                                .unwrap(),
                        );
                    }
                    started.elapsed()
                })
            });
        });

        group.bench_function(BenchmarkId::new("32_clients_fc03", label), |b| {
            let unit_ids = multi_units.clone();
            b.iter_custom(|iters| {
                runtime.block_on(async {
                    const CLIENTS: usize = 32;
                    let requests_per_client = (iters as usize).div_ceil(CLIENTS).max(1);
                    let mut clients = Vec::with_capacity(CLIENTS);
                    for _ in 0..CLIENTS {
                        clients.push(TcpRoundTripClient::connect(multi_addr).await);
                    }

                    let started = Instant::now();
                    let tasks = clients.into_iter().enumerate().map(|(idx, mut client)| {
                        let unit_ids = unit_ids.clone();
                        tokio::spawn(async move {
                            for step in 0..requests_per_client {
                                let unit_id = unit_ids[(idx + step) % unit_ids.len()];
                                let address = (step as u16) & 0x001f;
                                black_box(
                                    client
                                        .read_holding_register(unit_id, address)
                                        .await
                                        .unwrap(),
                                );
                            }
                        })
                    });

                    for task in tasks {
                        task.await.expect("client task should finish");
                    }

                    started.elapsed()
                })
            });
        });
    }

    group.finish();

    for harness in harnesses {
        runtime.block_on(harness.shutdown());
    }
}

fn benchmark_rtu_end_to_end(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("benchmark runtime");
    let mut group = c.benchmark_group("rtu_end_to_end");
    group.throughput(Throughput::Elements(1));

    let unit_ids = vec![1, 2, 3, 4];

    for (label, preset) in [
        ("default", RtuPerformancePreset::Default),
        ("high_throughput", RtuPerformancePreset::HighThroughput),
    ] {
        group.bench_function(BenchmarkId::new("channel_round_robin", label), |b| {
            let unit_ids = unit_ids.clone();
            b.iter_custom(|iters| {
                runtime.block_on(async {
                    let mut harness = RtuChannelHarness::start(preset, &unit_ids).await;
                    let _ = harness.read_holding_register(unit_ids[0], 0).await.unwrap();

                    let started = Instant::now();
                    for step in 0..iters {
                        let unit_id = unit_ids[step as usize % unit_ids.len()];
                        let address = (step as u16) & 0x001f;
                        black_box(
                            harness
                                .read_holding_register(unit_id, address)
                                .await
                                .unwrap(),
                        );
                    }
                    let elapsed = started.elapsed();

                    harness.shutdown().await;
                    elapsed
                })
            });
        });

        group.bench_function(BenchmarkId::new("tcp_bridge_round_robin", label), |b| {
            let unit_ids = unit_ids.clone();
            b.iter_custom(|iters| {
                runtime.block_on(async {
                    let mut harness = RtuTcpBridgeHarness::start(preset, &unit_ids).await;
                    let _ = harness.read_holding_register(unit_ids[0], 0).await.unwrap();

                    let started = Instant::now();
                    for step in 0..iters {
                        let unit_id = unit_ids[step as usize % unit_ids.len()];
                        let address = (step as u16) & 0x001f;
                        black_box(
                            harness
                                .read_holding_register(unit_id, address)
                                .await
                                .unwrap(),
                        );
                    }
                    let elapsed = started.elapsed();

                    harness.shutdown().await;
                    elapsed
                })
            });
        });
    }

    group.finish();
}

criterion_group!(
    transport_end_to_end,
    benchmark_tcp_end_to_end,
    benchmark_rtu_end_to_end
);
criterion_main!(transport_end_to_end);
