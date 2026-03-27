mod support;

use std::sync::Mutex;
use std::time::Instant;

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
static TRANSPORT_LATENCY_GUARD: Mutex<()> = Mutex::new(());

fn print_summary(label: &str, summary: LatencySummary) {
    println!(
        "{label}: avg={}us p50={}us p95={}us p99={}us",
        summary.avg_us, summary.p50_us, summary.p95_us, summary.p99_us
    );
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn report_transport_latency_profiles() {
    let _guard = TRANSPORT_LATENCY_GUARD.lock().unwrap();
    let tcp_units = vec![1, 2, 3, 4];
    for (label, preset) in [
        ("tcp_default", TcpPerformancePreset::Default),
        ("tcp_high_throughput", TcpPerformancePreset::HighThroughput),
    ] {
        let harness = TcpServerHarness::start(preset, &tcp_units).await;
        let mut client = TcpRoundTripClient::connect(harness.addr()).await;
        let _ = measure_tcp_client(&mut client, &tcp_units, WARMUP_REQUESTS).await;
        let summary = measure_tcp_client(&mut client, &tcp_units, LATENCY_SAMPLES).await;
        print_summary(label, summary);
        assert!(summary.p99_us > 0);
        let _ = measure_tcp_connection_churn(harness.addr(), &tcp_units, TCP_CHURN_WARMUP)
            .await
            .expect("TCP churn warmup should succeed");
        let churn_summary =
            measure_tcp_connection_churn(harness.addr(), &tcp_units, TCP_CHURN_SAMPLES)
                .await
                .expect("TCP churn measurement should succeed");
        let churn_label = format!("{label}_connection_churn");
        print_summary(&churn_label, churn_summary);
        assert!(churn_summary.p99_us > 0);
        harness.shutdown().await;
    }

    for (label, preset) in [
        ("rtu_channel_default", RtuPerformancePreset::Default),
        (
            "rtu_channel_high_throughput",
            RtuPerformancePreset::HighThroughput,
        ),
    ] {
        let mut harness = RtuChannelHarness::start(preset, &tcp_units).await;
        let _ = measure_rtu_channel(&mut harness, &tcp_units, WARMUP_REQUESTS).await;
        let summary = measure_rtu_channel(&mut harness, &tcp_units, LATENCY_SAMPLES).await;
        print_summary(label, summary);
        assert!(summary.p99_us > 0);
        harness.shutdown().await;
    }

    for (label, preset) in [
        ("rtu_tcp_bridge_default", RtuPerformancePreset::Default),
        (
            "rtu_tcp_bridge_high_throughput",
            RtuPerformancePreset::HighThroughput,
        ),
    ] {
        let mut harness = RtuTcpBridgeHarness::start(preset, &tcp_units).await;
        let _ = measure_rtu_bridge(&mut harness, &tcp_units, WARMUP_REQUESTS).await;
        let summary = measure_rtu_bridge(&mut harness, &tcp_units, LATENCY_SAMPLES).await;
        print_summary(label, summary);
        assert!(summary.p99_us > 0);
        harness.shutdown().await;
    }
}
