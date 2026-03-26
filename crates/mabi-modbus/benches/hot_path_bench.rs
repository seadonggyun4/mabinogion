use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use mabi_modbus::context::{DeviceContext, ServerContext, SharedAddressSpace};
use mabi_modbus::handler::HandlerRegistry;
use mabi_modbus::register::RegisterStore;
use mabi_modbus::registers::SparseRegisterStore;
use mabi_modbus::service::{
    execute_transport_request, ModbusService, ServiceOutcome, ServiceRequest,
    StandardModbusService, TransportServicePolicy, UnknownUnitBehavior,
};
use mabi_modbus::{RequestPdu, WordOrder};

fn dense_space() -> Arc<RegisterStore> {
    let space = Arc::new(RegisterStore::with_defaults());
    for addr in 0..512u16 {
        space.write_holding_register(addr, addr).unwrap();
    }
    space
}

fn sparse_space() -> Arc<SparseRegisterStore> {
    let space = Arc::new(SparseRegisterStore::with_defaults());
    for addr in 0..512u16 {
        space.write_holding_register(addr, addr).unwrap();
    }
    space
}

fn benchmark_read_write_hot_paths(c: &mut Criterion) {
    let dense = dense_space();
    let sparse = sparse_space();
    let mut group = c.benchmark_group("datastore_hot_paths");

    for (label, dense_like) in [
        ("dense", dense.clone() as Arc<dyn mabi_modbus::AddressSpace>),
        (
            "sparse",
            sparse.clone() as Arc<dyn mabi_modbus::AddressSpace>,
        ),
    ] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("single_read", label),
            &dense_like,
            |b, store| b.iter(|| black_box(store.read_holding_registers(100, 1).unwrap())),
        );
        group.bench_with_input(
            BenchmarkId::new("single_write", label),
            &dense_like,
            |b, store| {
                let mut value = 0u16;
                b.iter(|| {
                    value = value.wrapping_add(1);
                    black_box(store.write_holding_register(100, value).unwrap());
                })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("batch_read_125", label),
            &dense_like,
            |b, store| b.iter(|| black_box(store.read_holding_registers(0, 125).unwrap())),
        );
    }

    group.finish();
}

fn make_service_request(space: SharedAddressSpace) -> ServiceRequest {
    let server = ServerContext::new(space.clone());
    server.register(Arc::new(DeviceContext::new(
        1,
        "bench-unit",
        space,
        WordOrder::default(),
    )));

    let target = server.target_for_unit(1).expect("bench unit must exist");
    let pdu = RequestPdu::new(vec![0x03, 0x00, 0x00, 0x00, 0x01]).unwrap();
    ServiceRequest::new(1, 1, pdu, target)
}

fn benchmark_service_dispatch(c: &mut Criterion) {
    let dense = dense_space();
    let built_in_request = make_service_request(dense.clone());
    let registry_request = make_service_request(dense);
    let built_in = StandardModbusService::default();
    let registry = StandardModbusService::new(HandlerRegistry::with_defaults());
    let mut group = c.benchmark_group("service_dispatch");

    group.throughput(Throughput::Elements(1));
    group.bench_function("built_in_fast_path_fc03", |b| {
        b.iter(|| black_box(expect_reply(built_in.call(&built_in_request))))
    });
    group.bench_function("registry_dispatch_fc03", |b| {
        b.iter(|| black_box(expect_reply(registry.call(&registry_request))))
    });

    group.finish();
}

fn expect_reply(outcome: ServiceOutcome) -> Vec<u8> {
    match outcome {
        ServiceOutcome::Reply(response) => response.into_bytes(),
        ServiceOutcome::Ignore => panic!("benchmark request unexpectedly ignored"),
        ServiceOutcome::Exception(code) => panic!("benchmark request raised exception: {:?}", code),
    }
}

fn benchmark_transport_skeleton(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("benchmark runtime");
    let dense = dense_space();
    let server = ServerContext::new(dense.clone());
    server.register(Arc::new(DeviceContext::new(
        1,
        "bench-unit",
        dense,
        WordOrder::default(),
    )));
    let service = StandardModbusService::default();
    let read_pdu = [0x03, 0x00, 0x00, 0x00, 0x01];
    let write_pdu = [0x05, 0x00, 0x01, 0xFF, 0x00];
    let mut group = c.benchmark_group("transport_skeleton");

    group.throughput(Throughput::Elements(1));
    group.bench_function("unicast_fc03", |b| {
        b.iter(|| {
            runtime.block_on(execute_transport_request(
                &service,
                &server,
                1,
                1,
                black_box(&read_pdu),
                TransportServicePolicy::new(UnknownUnitBehavior::Ignore),
            ))
        })
    });
    group.bench_function("broadcast_fc05", |b| {
        b.iter(|| {
            runtime.block_on(execute_transport_request(
                &service,
                &server,
                0,
                1,
                black_box(&write_pdu),
                TransportServicePolicy::new(UnknownUnitBehavior::Ignore),
            ))
        })
    });

    group.finish();
}

criterion_group!(
    hot_path_benches,
    benchmark_read_write_hot_paths,
    benchmark_service_dispatch,
    benchmark_transport_skeleton
);
criterion_main!(hot_path_benches);
