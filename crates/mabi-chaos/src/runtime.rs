use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{json, Value as JsonValue};
use tokio::task::JoinHandle;

use mabi_core::device::DeviceInfo;
use mabi_core::types::DataPoint;
use mabi_core::value::Value;
use mabi_core::{Error as CoreError, Protocol};
use mabi_runtime::{DevicePort, DevicePortLayer, DynDevicePort, RuntimeExtensions};

use crate::config::{
    ChaosConfig, FaultConfig, FaultTypeConfig, ScheduleConfig, ScheduleFaultConfig,
};
use crate::context::OperationType;
use crate::device::{
    CorruptedDataFault, DeviceOfflineFault, SlowResponseFault, StateTransitionFault,
};
use crate::engine::ChaosEngine;
use crate::error::{ChaosError, ChaosResult};
use crate::fault::BoxedFault;
use crate::middleware::{ChaosMiddleware, MiddlewareResult};
use crate::network::{BandwidthFault, ConnectionFault, NetworkLatencyFault, PacketLossFault};
use crate::protocol::{ChecksumFault, MalformedPacketFault, ReorderFault, TimeoutFault};
use crate::scheduler::{ChaosEntry, ChaosEvent, ChaosSchedule, ChaosScheduler};

const SCHEDULE_FAULT_PREFIX: &str = "__fault_id=";

/// Same-process chaos runtime that can decorate runtime sessions.
pub struct ChaosRuntime {
    config: ChaosConfig,
    engine: Arc<ChaosEngine>,
    middleware: ChaosMiddleware,
    schedules: Vec<ChaosSchedule>,
    scheduler_tasks: Mutex<Vec<JoinHandle<()>>>,
    shutdown: Arc<tokio::sync::Notify>,
    protocol_configs: BTreeMap<String, JsonValue>,
}

impl ChaosRuntime {
    /// Compiles a chaos configuration into a runtime instance.
    pub fn new(config: ChaosConfig) -> ChaosResult<Self> {
        config.validate()?;

        let engine = Arc::new(ChaosEngine::new());
        let mut schedules = Vec::new();

        let mut fault_ids: Vec<_> = config.faults.keys().cloned().collect();
        fault_ids.sort();
        for fault_id in fault_ids {
            let fault = config
                .faults
                .get(&fault_id)
                .ok_or_else(|| ChaosError::Config(format!("missing fault '{}'", fault_id)))?;
            engine.register(&fault_id, build_fault(&fault_id, &fault.fault_type)?)?;
        }

        for (schedule_idx, schedule) in config.schedules.iter().enumerate() {
            if !schedule.enabled {
                continue;
            }
            schedules.push(compile_schedule(&engine, &config, schedule_idx, schedule)?);
        }

        if config.global.enabled {
            activate_static_faults(&engine, &config)?;
        }

        let middleware = ChaosMiddleware::from_arc(Arc::clone(&engine));
        let protocol_configs = build_protocol_configs(&config);

        Ok(Self {
            config,
            engine,
            middleware,
            schedules,
            scheduler_tasks: Mutex::new(Vec::new()),
            shutdown: Arc::new(tokio::sync::Notify::new()),
            protocol_configs,
        })
    }

    /// Returns the shared chaos engine.
    pub fn engine(&self) -> &Arc<ChaosEngine> {
        &self.engine
    }

    /// Builds runtime extensions for a shared runtime session.
    pub fn runtime_extensions(&self) -> RuntimeExtensions {
        let mut extensions = RuntimeExtensions::new();
        extensions.add_device_layer(Arc::new(ChaosDeviceLayer {
            middleware: self.middleware.clone(),
            exclude_patterns: self.config.global.exclude_patterns.clone(),
        }));
        for (protocol, config) in &self.protocol_configs {
            extensions.insert_protocol_config(protocol.clone(), config.clone());
        }
        extensions
    }

    /// Starts the chaos engine and any configured schedulers.
    pub async fn start(&self) -> ChaosResult<()> {
        self.engine.start().await?;

        let mut tasks = self.scheduler_tasks.lock();
        if !tasks.is_empty() {
            return Ok(());
        }

        for schedule in &self.schedules {
            let engine = Arc::clone(&self.engine);
            let shutdown = Arc::clone(&self.shutdown);
            let schedule = schedule.clone();
            tasks.push(tokio::spawn(async move {
                run_schedule(engine, schedule, shutdown).await;
            }));
        }

        Ok(())
    }

    /// Stops all scheduler tasks and then stops the engine.
    pub async fn stop(&self) -> ChaosResult<()> {
        self.shutdown.notify_waiters();

        let tasks = {
            let mut guard = self.scheduler_tasks.lock();
            std::mem::take(&mut *guard)
        };
        for task in tasks {
            let _ = task.await;
        }

        match self.engine.stop().await {
            Ok(()) => Ok(()),
            Err(ChaosError::EngineNotRunning) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

#[derive(Clone)]
struct ChaosDeviceLayer {
    middleware: ChaosMiddleware,
    exclude_patterns: Vec<String>,
}

impl DevicePortLayer for ChaosDeviceLayer {
    fn decorate(&self, protocol: Option<Protocol>, port: DynDevicePort) -> DynDevicePort {
        Arc::new(ChaosDevicePort {
            inner: port,
            middleware: self.middleware.clone(),
            protocol,
            exclude_patterns: self.exclude_patterns.clone(),
        })
    }
}

struct ChaosDevicePort {
    inner: DynDevicePort,
    middleware: ChaosMiddleware,
    protocol: Option<Protocol>,
    exclude_patterns: Vec<String>,
}

impl ChaosDevicePort {
    fn device_info(&self) -> DeviceInfo {
        self.inner.info()
    }

    fn protocol(&self) -> Protocol {
        self.protocol.unwrap_or_else(|| self.inner.info().protocol)
    }

    fn should_bypass(&self, device_id: &str) -> bool {
        self.exclude_patterns
            .iter()
            .any(|pattern| glob_match(pattern, device_id))
    }

    fn middleware_error(error: impl ToString) -> CoreError {
        CoreError::Protocol(error.to_string())
    }

    fn skipped_error(operation: &str, device_id: &str, point_id: &str) -> CoreError {
        CoreError::Protocol(format!(
            "chaos middleware skipped {} for {}.{}",
            operation, device_id, point_id
        ))
    }
}

#[async_trait]
impl DevicePort for ChaosDevicePort {
    fn info(&self) -> DeviceInfo {
        self.inner.info()
    }

    async fn start(&self) -> mabi_core::Result<()> {
        let info = self.device_info();
        if self.should_bypass(&info.id) {
            return self.inner.start().await;
        }
        match self
            .middleware
            .wrap_lifecycle(&info.id, self.protocol(), OperationType::Start)
            .await
            .map_err(Self::middleware_error)?
        {
            MiddlewareResult::Proceed(_) | MiddlewareResult::Delayed { .. } => {
                self.inner.start().await
            }
            MiddlewareResult::Skip => Ok(()),
            MiddlewareResult::Error { message, .. } => Err(CoreError::Protocol(message)),
        }
    }

    async fn stop(&self) -> mabi_core::Result<()> {
        let info = self.device_info();
        if self.should_bypass(&info.id) {
            return self.inner.stop().await;
        }
        match self
            .middleware
            .wrap_lifecycle(&info.id, self.protocol(), OperationType::Stop)
            .await
            .map_err(Self::middleware_error)?
        {
            MiddlewareResult::Proceed(_) | MiddlewareResult::Delayed { .. } => {
                self.inner.stop().await
            }
            MiddlewareResult::Skip => Ok(()),
            MiddlewareResult::Error { message, .. } => Err(CoreError::Protocol(message)),
        }
    }

    async fn read(&self, point_id: &str) -> mabi_core::Result<DataPoint> {
        let info = self.device_info();
        if self.should_bypass(&info.id) {
            return self.inner.read(point_id).await;
        }

        let ctx = match self
            .middleware
            .wrap_read(&info.id, self.protocol(), point_id)
            .await
            .map_err(Self::middleware_error)?
        {
            MiddlewareResult::Proceed(ctx) | MiddlewareResult::Delayed { result: ctx, .. } => ctx,
            MiddlewareResult::Skip => {
                return Err(Self::skipped_error("read", &info.id, point_id));
            }
            MiddlewareResult::Error { message, .. } => {
                return Err(CoreError::Protocol(message));
            }
        };

        let point = self.inner.read(point_id).await?;
        match self
            .middleware
            .process_response(ctx, vec![point])
            .await
            .map_err(Self::middleware_error)?
        {
            MiddlewareResult::Proceed(mut points)
            | MiddlewareResult::Delayed {
                result: mut points, ..
            } => points.pop().ok_or_else(|| {
                CoreError::Protocol(format!(
                    "chaos middleware produced no point for {}.{}",
                    info.id, point_id
                ))
            }),
            MiddlewareResult::Skip => Err(Self::skipped_error("response", &info.id, point_id)),
            MiddlewareResult::Error { message, .. } => Err(CoreError::Protocol(message)),
        }
    }

    async fn write(&self, point_id: &str, value: Value) -> mabi_core::Result<()> {
        let info = self.device_info();
        if self.should_bypass(&info.id) {
            return self.inner.write(point_id, value).await;
        }

        let ctx = match self
            .middleware
            .wrap_write(&info.id, self.protocol(), point_id, value)
            .await
            .map_err(Self::middleware_error)?
        {
            MiddlewareResult::Proceed(ctx) | MiddlewareResult::Delayed { result: ctx, .. } => ctx,
            MiddlewareResult::Skip => {
                return Err(Self::skipped_error("write", &info.id, point_id));
            }
            MiddlewareResult::Error { message, .. } => {
                return Err(CoreError::Protocol(message));
            }
        };

        let write_value = match ctx.operation {
            OperationType::Write { value, .. } => value,
            _ => Value::Null,
        };
        self.inner.write(point_id, write_value).await
    }
}

fn build_fault(fault_id: &str, config: &FaultTypeConfig) -> ChaosResult<BoxedFault> {
    Ok(match config {
        FaultTypeConfig::Latency(config) => {
            Box::new(NetworkLatencyFault::new(fault_id, config.clone()))
        }
        FaultTypeConfig::PacketLoss(config) => {
            Box::new(PacketLossFault::new(fault_id, config.clone()))
        }
        FaultTypeConfig::Connection(config) => {
            Box::new(ConnectionFault::new(fault_id, config.clone()))
        }
        FaultTypeConfig::Bandwidth(config) => {
            Box::new(BandwidthFault::new(fault_id, config.clone()))
        }
        FaultTypeConfig::Offline(config) => {
            Box::new(DeviceOfflineFault::new(fault_id, config.clone()))
        }
        FaultTypeConfig::SlowResponse(config) => {
            Box::new(SlowResponseFault::new(fault_id, config.clone()))
        }
        FaultTypeConfig::CorruptedData(config) => {
            Box::new(CorruptedDataFault::new(fault_id, config.clone()))
        }
        FaultTypeConfig::StateTransition(config) => {
            Box::new(StateTransitionFault::new(fault_id, config.clone()))
        }
        FaultTypeConfig::Malformed(config) => {
            Box::new(MalformedPacketFault::new(fault_id, config.clone()))
        }
        FaultTypeConfig::Checksum(config) => Box::new(ChecksumFault::new(fault_id, config.clone())),
        FaultTypeConfig::Timeout(config) => Box::new(TimeoutFault::new(fault_id, config.clone())),
        FaultTypeConfig::Reorder(config) => Box::new(ReorderFault::new(fault_id, config.clone())),
    })
}

fn activation_targets(config: &ChaosConfig, fault: &FaultConfig) -> Vec<String> {
    if !fault.targets.is_empty() {
        fault.targets.clone()
    } else {
        config.global.target_patterns.clone()
    }
}

fn activate_static_faults(engine: &Arc<ChaosEngine>, config: &ChaosConfig) -> ChaosResult<()> {
    let mut fault_ids: Vec<_> = config.faults.keys().cloned().collect();
    fault_ids.sort();

    for fault_id in fault_ids {
        let Some(fault) = config.faults.get(&fault_id) else {
            continue;
        };
        if !fault.enabled {
            continue;
        }

        let targets = activation_targets(config, fault);
        if targets.is_empty() {
            engine.registry().activate_globally(&fault_id)?;
        } else {
            for target in targets {
                engine.registry().activate(&fault_id, target)?;
            }
        }
    }

    Ok(())
}

fn compile_schedule(
    engine: &Arc<ChaosEngine>,
    config: &ChaosConfig,
    schedule_idx: usize,
    schedule: &ScheduleConfig,
) -> ChaosResult<ChaosSchedule> {
    let mut compiled = ChaosSchedule::new(&schedule.name);
    compiled.description = schedule.description.clone().unwrap_or_default();
    compiled.loop_schedule = schedule.loop_schedule;
    compiled.total_duration_secs = schedule.total_duration_secs;

    for (entry_idx, entry) in schedule.entries.iter().enumerate() {
        let fault_id = match &entry.fault {
            ScheduleFaultConfig::Reference { fault_id } => {
                if !config.faults.contains_key(fault_id) {
                    return Err(ChaosError::Config(format!(
                        "schedule '{}' references unknown fault '{}'",
                        schedule.name, fault_id
                    )));
                }
                fault_id.clone()
            }
            ScheduleFaultConfig::Inline(fault_type) => {
                let fault_id = format!("schedule:{}:{}:{}", schedule_idx, schedule.name, entry_idx);
                if !engine.registry().contains(&fault_id) {
                    engine.register(&fault_id, build_fault(&fault_id, fault_type)?)?;
                }
                fault_id
            }
        };

        let mut compiled_entry = entry.to_entry()?;
        compiled_entry.description = format!("{}{}", SCHEDULE_FAULT_PREFIX, fault_id);
        compiled.add_entry(compiled_entry);
    }

    Ok(compiled)
}

async fn run_schedule(
    engine: Arc<ChaosEngine>,
    schedule: ChaosSchedule,
    shutdown: Arc<tokio::sync::Notify>,
) {
    let mut scheduler = ChaosScheduler::new(schedule);
    scheduler.start();

    loop {
        tokio::select! {
            _ = shutdown.notified() => break,
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }

        for event in scheduler.tick() {
            match event {
                ChaosEvent::Started(entry) => {
                    if let Some(fault_id) = scheduled_fault_id(&entry) {
                        activate_schedule_entry(&engine, fault_id, &entry).await;
                    }
                }
                ChaosEvent::Ended(entry) => {
                    if let Some(fault_id) = scheduled_fault_id(&entry) {
                        deactivate_schedule_entry(&engine, fault_id, &entry).await;
                    }
                }
                ChaosEvent::ScheduleCompleted => break,
                ChaosEvent::ScheduleLooped => {}
            }
        }
    }
}

fn scheduled_fault_id(entry: &ChaosEntry) -> Option<&str> {
    if let Some(fault_id) = entry.description.strip_prefix(SCHEDULE_FAULT_PREFIX) {
        return Some(fault_id);
    }
    None
}

async fn activate_schedule_entry(engine: &Arc<ChaosEngine>, fault_id: &str, entry: &ChaosEntry) {
    if entry.targets.is_empty() {
        let _ = engine.enable_globally(fault_id).await;
    } else {
        for target in &entry.targets {
            let _ = engine.enable(fault_id, target.clone()).await;
        }
    }
}

async fn deactivate_schedule_entry(engine: &Arc<ChaosEngine>, fault_id: &str, entry: &ChaosEntry) {
    if entry.targets.is_empty() {
        let _ = engine.disable_globally(fault_id).await;
    } else {
        for target in &entry.targets {
            let _ = engine.disable(fault_id, target).await;
        }
    }
}

fn build_protocol_configs(config: &ChaosConfig) -> BTreeMap<String, JsonValue> {
    let mut configs = BTreeMap::new();

    if let Some(modbus) = build_modbus_runtime_config(config) {
        configs.insert("modbus".to_string(), modbus);
    }

    configs
}

fn build_modbus_runtime_config(config: &ChaosConfig) -> Option<JsonValue> {
    let mut faults = Vec::new();
    let mut connection_disruption = None;

    let mut ids: Vec<_> = config.faults.keys().cloned().collect();
    ids.sort();

    for fault_id in ids {
        let Some(fault) = config.faults.get(&fault_id) else {
            continue;
        };
        if !fault.enabled {
            continue;
        }

        let targets = activation_targets(config, fault);
        let unit_ids = modbus_unit_ids(&targets);
        let target = json!({
            "unit_ids": unit_ids,
            "probability": fault.probability,
        });

        match &fault.fault_type {
            FaultTypeConfig::Latency(latency) => faults.push(json!({
                "type": "delayed_response",
                "target": target,
                "config": {
                    "delay_ms": latency.base_ms,
                    "jitter_ms": latency.jitter_ms,
                }
            })),
            FaultTypeConfig::SlowResponse(slow) => faults.push(json!({
                "type": "delayed_response",
                "target": target,
                "config": {
                    "delay_ms": slow.base_delay_ms,
                    "jitter_ms": slow.max_additional_delay_ms,
                }
            })),
            FaultTypeConfig::PacketLoss(_)
            | FaultTypeConfig::Offline(_)
            | FaultTypeConfig::Timeout(_) => faults.push(json!({
                "type": "no_response",
                "target": target,
                "config": {}
            })),
            FaultTypeConfig::Checksum(_) => faults.push(json!({
                "type": "crc_corruption",
                "target": target,
                "config": {
                    "crc_mode": "invert",
                }
            })),
            FaultTypeConfig::Malformed(_) => faults.push(json!({
                "type": "truncated_response",
                "target": target,
                "config": {
                    "truncation_mode": "percentage",
                    "truncation_percentage": 0.5,
                }
            })),
            FaultTypeConfig::Connection(connection) if connection_disruption.is_none() => {
                connection_disruption = Some(json!({
                    "drop_after_requests": connection.max_disconnections.max(1),
                    "close_delay": connection.disconnect_duration_ms.map(|duration_ms| json!({
                        "secs": duration_ms / 1000,
                        "nanos": ((duration_ms % 1000) * 1_000_000) as u32
                    })),
                    "use_rst": matches!(connection.mode, crate::network::DisconnectMode::Reset),
                }));
            }
            _ => {}
        }
    }

    if faults.is_empty() && connection_disruption.is_none() {
        return None;
    }

    Some(json!({
        "fault_injection": {
            "enabled": !faults.is_empty(),
            "faults": faults,
        },
        "connection_disruption": connection_disruption,
    }))
}

fn modbus_unit_ids(targets: &[String]) -> Vec<u8> {
    let mut unit_ids = Vec::new();
    for target in targets {
        if let Some(unit_id) = target
            .strip_prefix("modbus-")
            .and_then(|value| value.parse::<u8>().ok())
        {
            unit_ids.push(unit_id);
        }
    }
    unit_ids
}

fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.starts_with('*') && pattern.ends_with('*') {
        return text.contains(&pattern[1..pattern.len() - 1]);
    }
    if let Some(pattern) = pattern.strip_prefix('*') {
        return text.ends_with(pattern);
    }
    if let Some(pattern) = pattern.strip_suffix('*') {
        return text.starts_with(pattern);
    }
    pattern == text
}
