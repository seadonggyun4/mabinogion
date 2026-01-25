//! Benchmarks for scalability module components.
//!
//! Run with: `cargo bench -p trap-sim-modbus`
//!
//! ## Benchmark Categories
//!
//! 1. **Connection Pool**: Registration, lookup, and unregistration performance
//! 2. **Batch Processor**: Throughput and latency under various loads
//! 3. **Resource Limiter**: Check and acquire performance
//! 4. **Profiler**: Recording overhead

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use mabi_modbus::scalability::{
    ShardedConnectionPool, ConnectionPoolConfig,
    BatchProcessor, BatchProcessorConfig, BatchRequest, BatchHandler, BatchError,
    ResourceLimiter, ResourceLimiterConfig, BackpressureStrategyConfig,
    PerformanceProfiler, ProfilerConfig,
    CoalescingConfig,
};

// =============================================================================
// Test Helpers
// =============================================================================

fn make_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port)
}

fn make_pool_config(max_connections: usize, shard_count: usize) -> ConnectionPoolConfig {
    ConnectionPoolConfig {
        max_connections,
        shard_count,
        connections_per_shard: max_connections / shard_count + 1,
        idle_timeout: Duration::from_secs(300),
        health_check_interval: Duration::from_secs(60),
        enable_metrics: false, // Disable for pure performance testing
    }
}

/// Echo handler for batch processor benchmarks
struct EchoHandler;

impl BatchHandler<Vec<u8>> for EchoHandler {
    fn process(&self, request: &BatchRequest<Vec<u8>>) -> Result<Vec<u8>, BatchError> {
        Ok(request.payload.clone())
    }
}

// =============================================================================
// Connection Pool Benchmarks
// =============================================================================

fn bench_connection_pool_register(c: &mut Criterion) {
    let mut group = c.benchmark_group("connection_pool_register");

    for shard_count in [8, 32, 64, 128] {
        group.bench_with_input(
            BenchmarkId::new("shards", shard_count),
            &shard_count,
            |b, &shard_count| {
                let config = make_pool_config(100_000, shard_count);
                let pool = ShardedConnectionPool::new(config);
                let mut port = 1000u16;

                b.iter(|| {
                    let addr = make_addr(port);
                    port = port.wrapping_add(1);
                    if port < 1000 {
                        port = 1000;
                    }
                    let handle = pool.try_register(addr);
                    black_box(handle)
                });
            },
        );
    }

    group.finish();
}

fn bench_connection_pool_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("connection_pool_lookup");

    for num_connections in [100, 1_000, 10_000] {
        let config = make_pool_config(100_000, 64);
        let pool = ShardedConnectionPool::new(config);

        // Pre-populate pool
        let handles: Vec<_> = (0..num_connections)
            .map(|i| pool.try_register(make_addr(1000 + i as u16)).unwrap())
            .collect();

        let lookup_ids: Vec<_> = handles.iter().map(|h| h.id()).collect();

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("connections", num_connections),
            &lookup_ids,
            |b, ids| {
                let mut idx = 0;
                b.iter(|| {
                    let id = ids[idx % ids.len()];
                    idx += 1;
                    let info = pool.get(id);
                    black_box(info)
                });
            },
        );

        drop(handles);
    }

    group.finish();
}

fn bench_connection_pool_mixed(c: &mut Criterion) {
    let mut group = c.benchmark_group("connection_pool_mixed");

    let config = make_pool_config(100_000, 64);
    let pool = ShardedConnectionPool::new(config);

    // Pre-populate with some connections
    let _handles: Vec<_> = (0..1000)
        .map(|i| pool.try_register(make_addr(1000 + i)).unwrap())
        .collect();

    group.throughput(Throughput::Elements(4)); // 4 ops per iteration
    group.bench_function("register_lookup_record", |b| {
        let mut port = 10000u16;
        b.iter(|| {
            // Register
            let addr = make_addr(port);
            port = port.wrapping_add(1);
            if let Some(handle) = pool.try_register(addr) {
                // Lookup
                let _ = black_box(pool.get(handle.id()));

                // Record request
                handle.record_success(100, 200);

                // Handle dropped, unregisters automatically
            }
        });
    });

    group.finish();
}

// =============================================================================
// Batch Processor Benchmarks
// =============================================================================

fn bench_batch_processor_submit(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("batch_processor_submit");

    for batch_size in [10, 50, 100, 200] {
        let config = BatchProcessorConfig {
            enabled: true,
            batch_size,
            batch_timeout: Duration::from_millis(10),
            max_pending: 10_000,
            coalescing: CoalescingConfig::disabled(),
        };

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("batch_size", batch_size),
            &config,
            |b, config| {
                let processor = BatchProcessor::<Vec<u8>>::new(config.clone());
                let payload = vec![0x03, 0x00, 0x00, 0x00, 0x0A];

                b.iter(|| {
                    let (request, _rx) = BatchRequest::new(
                        payload.clone(),
                        1,
                        0x03,
                        0,
                        10,
                    );
                    let _ = black_box(processor.submit(request));
                });
            },
        );
    }

    group.finish();
}

fn bench_batch_processor_flush(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("batch_processor_flush");

    for batch_size in [10, 50, 100] {
        let config = BatchProcessorConfig {
            enabled: true,
            batch_size,
            batch_timeout: Duration::from_millis(100),
            max_pending: 10_000,
            coalescing: CoalescingConfig::disabled(),
        };

        group.throughput(Throughput::Elements(batch_size as u64));
        group.bench_with_input(
            BenchmarkId::new("batch_size", batch_size),
            &(config, batch_size),
            |b, (config, size)| {
                let processor = BatchProcessor::<Vec<u8>>::new(config.clone());
                let handler = EchoHandler;
                let payload = vec![0x03, 0x00, 0x00, 0x00, 0x0A];

                b.iter(|| {
                    // Submit batch_size requests
                    for i in 0..*size {
                        let (request, _rx) = BatchRequest::new(
                            payload.clone(),
                            1,
                            0x03,
                            i as u16,
                            1,
                        );
                        let _ = processor.submit(request);
                    }

                    // Flush
                    rt.block_on(async {
                        let processed = processor.flush(&handler).await;
                        black_box(processed)
                    })
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// Resource Limiter Benchmarks
// =============================================================================

fn bench_resource_limiter_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("resource_limiter_check");

    for strategy in [
        ("none", BackpressureStrategyConfig::None),
        ("gradual", BackpressureStrategyConfig::Gradual),
        ("adaptive", BackpressureStrategyConfig::Adaptive),
        ("aggressive", BackpressureStrategyConfig::Aggressive),
    ] {
        let config = ResourceLimiterConfig {
            max_memory_bytes: 2 * 1024 * 1024 * 1024,
            max_connections: 10_000,
            max_requests_per_second: 100_000,
            backpressure_threshold: 0.8,
            strategy: strategy.1,
        };

        let limiter = ResourceLimiter::new(config);
        // Set some utilization
        limiter.set_memory_used(1024 * 1024 * 1024); // 50%
        // Add 5000 connections
        for _ in 0..5000 {
            limiter.add_connection();
        }

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("strategy", strategy.0),
            &limiter,
            |b, limiter| {
                b.iter(|| {
                    let result = limiter.check();
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

fn bench_resource_limiter_acquire(c: &mut Criterion) {
    let mut group = c.benchmark_group("resource_limiter_acquire");

    let config = ResourceLimiterConfig {
        max_memory_bytes: 2 * 1024 * 1024 * 1024,
        max_connections: 10_000,
        max_requests_per_second: 1_000_000, // High rate for benchmarking
        backpressure_threshold: 0.8,
        strategy: BackpressureStrategyConfig::Adaptive,
    };

    let limiter = ResourceLimiter::new(config);

    group.throughput(Throughput::Elements(1));
    group.bench_function("try_acquire", |b| {
        b.iter(|| {
            let result = limiter.try_acquire();
            black_box(result)
        });
    });

    group.finish();
}

// =============================================================================
// Profiler Benchmarks
// =============================================================================

fn bench_profiler_record(c: &mut Criterion) {
    let mut group = c.benchmark_group("profiler_record");

    for sample_rate in [1.0, 0.1, 0.01, 0.001] {
        let config = ProfilerConfig {
            enabled: true,
            sample_rate,
            histogram_buckets: vec![50, 100, 250, 500, 1000, 2500, 5000, 10000],
            report_interval: Duration::from_secs(60),
            track_memory: false,
            track_cpu: false,
        };

        let profiler = PerformanceProfiler::new(config);

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("sample_rate", format!("{:.1}%", sample_rate * 100.0)),
            &profiler,
            |b, profiler| {
                b.iter(|| {
                    profiler.record_latency(Duration::from_micros(500));
                });
            },
        );
    }

    group.finish();
}

fn bench_profiler_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("profiler_snapshot");

    let profiler = PerformanceProfiler::with_defaults();

    // Pre-record some data
    for _ in 0..10_000 {
        profiler.record_latency(Duration::from_micros(rand::random::<u64>() % 10_000));
    }

    group.bench_function("snapshot", |b| {
        b.iter(|| {
            let snapshot = profiler.snapshot();
            black_box(snapshot)
        });
    });

    group.bench_function("report", |b| {
        b.iter(|| {
            let report = profiler.report();
            black_box(report)
        });
    });

    group.finish();
}

fn bench_latency_histogram(c: &mut Criterion) {
    use mabi_modbus::scalability::LatencyHistogram;

    let mut group = c.benchmark_group("latency_histogram");

    let histogram = LatencyHistogram::with_defaults();

    group.throughput(Throughput::Elements(1));
    group.bench_function("record", |b| {
        let mut value = 0u64;
        b.iter(|| {
            value = (value + 137) % 100_000; // Pseudo-random
            histogram.record(value);
        });
    });

    // Pre-populate for percentile benchmarks
    for i in 0..100_000 {
        histogram.record(i % 50_000);
    }

    group.bench_function("percentile_p99", |b| {
        b.iter(|| {
            let p99 = histogram.p99();
            black_box(p99)
        });
    });

    group.finish();
}

// =============================================================================
// Criterion Groups
// =============================================================================

criterion_group!(
    connection_pool_benches,
    bench_connection_pool_register,
    bench_connection_pool_lookup,
    bench_connection_pool_mixed,
);

criterion_group!(
    batch_processor_benches,
    bench_batch_processor_submit,
    bench_batch_processor_flush,
);

criterion_group!(
    resource_limiter_benches,
    bench_resource_limiter_check,
    bench_resource_limiter_acquire,
);

criterion_group!(
    profiler_benches,
    bench_profiler_record,
    bench_profiler_snapshot,
    bench_latency_histogram,
);

criterion_main!(
    connection_pool_benches,
    batch_processor_benches,
    resource_limiter_benches,
    profiler_benches,
);
