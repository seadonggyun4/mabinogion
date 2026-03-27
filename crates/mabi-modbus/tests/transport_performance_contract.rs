#![cfg(feature = "performance-tests")]

mod support;

use std::future::Future;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use mabi_modbus::rtu::PerformancePreset as RtuPerformancePreset;
use mabi_modbus::tcp::PerformancePreset as TcpPerformancePreset;
use support::transport_harness::{
    measure_tcp_connection_churn, summarize_latencies, LatencySummary, RtuChannelHarness,
    RtuTcpBridgeHarness, TcpRoundTripClient, TcpServerHarness,
};

const WARMUP_REQUESTS: usize = 200;
const LATENCY_SAMPLES: usize = 2_000;
const TCP_CHURN_WARMUP: usize = 24;
const TCP_CHURN_SAMPLES: usize = 128;
const RUNS: usize = 3;
const TCP_REQUEST_NOISE_TOLERANCE_US: u64 = 25;
const TCP_CHURN_NOISE_TOLERANCE_US: u64 = 25;
const MEASUREMENT_TIMEOUT: Duration = Duration::from_secs(15);
static TRANSPORT_PERF_GUARD: Mutex<()> = Mutex::new(());

fn median(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn summarize_series(label: &str, summaries: &[LatencySummary]) -> (u64, u64) {
    let median_avg = median(summaries.iter().map(|summary| summary.avg_us).collect());
    let median_p99 = median(summaries.iter().map(|summary| summary.p99_us).collect());
    println!("{label}: median_avg={median_avg}us median_p99={median_p99}us");
    (median_avg, median_p99)
}

async fn run_measurement<F>(label: &str, future: F) -> LatencySummary
where
    F: Future<Output = LatencySummary>,
{
    println!("starting {label}");
    let summary = tokio::time::timeout(MEASUREMENT_TIMEOUT, future)
        .await
        .unwrap_or_else(|_| panic!("{label} exceeded {:?}", MEASUREMENT_TIMEOUT));
    println!(
        "finished {label}: avg={}us p99={}us",
        summary.avg_us, summary.p99_us
    );
    summary
}

async fn measure_tcp_client(
    client: &mut TcpRoundTripClient,
    unit_ids: &[u8],
    samples: usize,
) -> LatencySummary {
    let mut latencies = Vec::with_capacity(samples);
    for step in 0..samples {
        let started = Instant::now();
        let unit_id = unit_ids[step % unit_ids.len()];
        let address = (step as u16) & 0x001f;
        let _ = client
            .read_holding_register(unit_id, address)
            .await
            .expect("TCP request should succeed");
        latencies.push(started.elapsed().as_micros() as u64);
    }
    summarize_latencies(latencies.as_mut_slice())
}

async fn measure_rtu_channel(
    harness: &mut RtuChannelHarness,
    unit_ids: &[u8],
    samples: usize,
) -> LatencySummary {
    let mut latencies = Vec::with_capacity(samples);
    for step in 0..samples {
        let started = Instant::now();
        let unit_id = unit_ids[step % unit_ids.len()];
        let address = (step as u16) & 0x001f;
        let _ = harness
            .read_holding_register(unit_id, address)
            .await
            .expect("RTU channel request should succeed");
        latencies.push(started.elapsed().as_micros() as u64);
    }
    summarize_latencies(latencies.as_mut_slice())
}

async fn measure_rtu_bridge(
    harness: &mut RtuTcpBridgeHarness,
    unit_ids: &[u8],
    samples: usize,
) -> LatencySummary {
    let mut latencies = Vec::with_capacity(samples);
    for step in 0..samples {
        let started = Instant::now();
        let unit_id = unit_ids[step % unit_ids.len()];
        let address = (step as u16) & 0x001f;
        let _ = harness
            .read_holding_register(unit_id, address)
            .await
            .expect("RTU bridge request should succeed");
        latencies.push(started.elapsed().as_micros() as u64);
    }
    summarize_latencies(latencies.as_mut_slice())
}

async fn tcp_summary(preset: TcpPerformancePreset, unit_ids: &[u8]) -> LatencySummary {
    let harness = TcpServerHarness::start(preset, unit_ids).await;
    let mut client = TcpRoundTripClient::connect(harness.addr()).await;
    let _ = measure_tcp_client(&mut client, unit_ids, WARMUP_REQUESTS).await;
    let summary = measure_tcp_client(&mut client, unit_ids, LATENCY_SAMPLES).await;
    harness.shutdown().await;
    summary
}

async fn tcp_connection_churn_summary(
    preset: TcpPerformancePreset,
    unit_ids: &[u8],
) -> LatencySummary {
    let harness = TcpServerHarness::start(preset, unit_ids).await;
    let _ = measure_tcp_connection_churn(harness.addr(), unit_ids, TCP_CHURN_WARMUP)
        .await
        .expect("TCP churn warmup should succeed");
    let summary = measure_tcp_connection_churn(harness.addr(), unit_ids, TCP_CHURN_SAMPLES)
        .await
        .expect("TCP churn measurement should succeed");
    harness.shutdown().await;
    summary
}

async fn rtu_channel_summary(preset: RtuPerformancePreset, unit_ids: &[u8]) -> LatencySummary {
    let mut harness = RtuChannelHarness::start(preset, unit_ids).await;
    let _ = measure_rtu_channel(&mut harness, unit_ids, WARMUP_REQUESTS).await;
    let summary = measure_rtu_channel(&mut harness, unit_ids, LATENCY_SAMPLES).await;
    harness.shutdown().await;
    summary
}

async fn rtu_bridge_summary(preset: RtuPerformancePreset, unit_ids: &[u8]) -> LatencySummary {
    let mut harness = RtuTcpBridgeHarness::start(preset, unit_ids).await;
    let _ = measure_rtu_bridge(&mut harness, unit_ids, WARMUP_REQUESTS).await;
    let summary = measure_rtu_bridge(&mut harness, unit_ids, LATENCY_SAMPLES).await;
    harness.shutdown().await;
    summary
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn high_throughput_is_not_slower_than_default_on_warm_transport_paths() {
    let _guard = TRANSPORT_PERF_GUARD.lock().unwrap();
    let unit_ids = vec![1, 2, 3, 4];

    let mut tcp_default = Vec::with_capacity(RUNS);
    let mut tcp_high = Vec::with_capacity(RUNS);
    let mut tcp_churn_default = Vec::with_capacity(RUNS);
    let mut tcp_churn_high = Vec::with_capacity(RUNS);
    let mut rtu_channel_default = Vec::with_capacity(RUNS);
    let mut rtu_channel_high = Vec::with_capacity(RUNS);
    let mut rtu_bridge_default = Vec::with_capacity(RUNS);
    let mut rtu_bridge_high = Vec::with_capacity(RUNS);

    for run in 0..RUNS {
        tcp_default.push(
            run_measurement(
                &format!("run{} tcp_default", run + 1),
                tcp_summary(TcpPerformancePreset::Default, &unit_ids),
            )
            .await,
        );
        tcp_high.push(
            run_measurement(
                &format!("run{} tcp_high", run + 1),
                tcp_summary(TcpPerformancePreset::HighThroughput, &unit_ids),
            )
            .await,
        );
        tcp_churn_default.push(
            run_measurement(
                &format!("run{} tcp_churn_default", run + 1),
                tcp_connection_churn_summary(TcpPerformancePreset::Default, &unit_ids),
            )
            .await,
        );
        tcp_churn_high.push(
            run_measurement(
                &format!("run{} tcp_churn_high", run + 1),
                tcp_connection_churn_summary(TcpPerformancePreset::HighThroughput, &unit_ids),
            )
            .await,
        );
        rtu_channel_default.push(
            run_measurement(
                &format!("run{} rtu_channel_default", run + 1),
                rtu_channel_summary(RtuPerformancePreset::Default, &unit_ids),
            )
            .await,
        );
        rtu_channel_high.push(
            run_measurement(
                &format!("run{} rtu_channel_high", run + 1),
                rtu_channel_summary(RtuPerformancePreset::HighThroughput, &unit_ids),
            )
            .await,
        );
        rtu_bridge_default.push(
            run_measurement(
                &format!("run{} rtu_bridge_default", run + 1),
                rtu_bridge_summary(RtuPerformancePreset::Default, &unit_ids),
            )
            .await,
        );
        rtu_bridge_high.push(
            run_measurement(
                &format!("run{} rtu_bridge_high", run + 1),
                rtu_bridge_summary(RtuPerformancePreset::HighThroughput, &unit_ids),
            )
            .await,
        );
    }

    let (tcp_default_avg, tcp_default_p99) = summarize_series("tcp_default", &tcp_default);
    let (tcp_high_avg, tcp_high_p99) = summarize_series("tcp_high", &tcp_high);
    let (tcp_churn_default_avg, tcp_churn_default_p99) =
        summarize_series("tcp_churn_default", &tcp_churn_default);
    let (tcp_churn_high_avg, tcp_churn_high_p99) =
        summarize_series("tcp_churn_high", &tcp_churn_high);
    let (rtu_channel_default_avg, rtu_channel_default_p99) =
        summarize_series("rtu_channel_default", &rtu_channel_default);
    let (rtu_channel_high_avg, rtu_channel_high_p99) =
        summarize_series("rtu_channel_high", &rtu_channel_high);
    let (rtu_bridge_default_avg, rtu_bridge_default_p99) =
        summarize_series("rtu_bridge_default", &rtu_bridge_default);
    let (rtu_bridge_high_avg, rtu_bridge_high_p99) =
        summarize_series("rtu_bridge_high", &rtu_bridge_high);

    assert!(
        tcp_high_avg <= tcp_default_avg + TCP_REQUEST_NOISE_TOLERANCE_US
            && tcp_high_p99 <= tcp_default_p99 + TCP_REQUEST_NOISE_TOLERANCE_US,
        "TCP high_throughput regressed beyond jitter tolerance: default avg/p99={tcp_default_avg}/{tcp_default_p99}, high avg/p99={tcp_high_avg}/{tcp_high_p99}, tolerance={TCP_REQUEST_NOISE_TOLERANCE_US}us"
    );
    assert!(
        tcp_churn_high_avg <= tcp_churn_default_avg + TCP_CHURN_NOISE_TOLERANCE_US
            && tcp_churn_high_p99 <= tcp_churn_default_p99 + TCP_CHURN_NOISE_TOLERANCE_US,
        "TCP connection churn high_throughput regressed beyond jitter tolerance: default avg/p99={tcp_churn_default_avg}/{tcp_churn_default_p99}, high avg/p99={tcp_churn_high_avg}/{tcp_churn_high_p99}, tolerance={TCP_CHURN_NOISE_TOLERANCE_US}us"
    );
    assert!(
        rtu_channel_high_avg <= rtu_channel_default_avg
            && rtu_channel_high_p99 <= rtu_channel_default_p99,
        "RTU channel high_throughput regressed: default avg/p99={rtu_channel_default_avg}/{rtu_channel_default_p99}, high avg/p99={rtu_channel_high_avg}/{rtu_channel_high_p99}"
    );
    assert!(
        rtu_bridge_high_avg <= rtu_bridge_default_avg
            && rtu_bridge_high_p99 <= rtu_bridge_default_p99,
        "RTU tcp_bridge high_throughput regressed: default avg/p99={rtu_bridge_default_avg}/{rtu_bridge_default_p99}, high avg/p99={rtu_bridge_high_avg}/{rtu_bridge_high_p99}"
    );
}
