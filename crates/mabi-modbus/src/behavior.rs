//! Deterministic behavior-layer runtime for controller-visible device ports.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parking_lot::Mutex;

use mabi_core::device::DeviceInfo;
use mabi_core::types::{DataPoint, DataPointDef, Quality};
use mabi_core::value::Value;
use mabi_runtime::{DevicePort, DevicePortLayer, DynDevicePort};

use crate::simulator::{
    ActionDefinition, BehaviorCondition, BehaviorConditionOperator, BehaviorTrigger,
    CompiledModbusSession,
};

#[derive(Clone)]
struct ResolvedBehavior {
    name: String,
    point_id: String,
    trigger: BehaviorTrigger,
    condition: Option<BehaviorCondition>,
    interval_ms: Option<u64>,
    actions: Vec<ActionDefinition>,
}

#[derive(Default)]
struct BehaviorState {
    startup_applied: bool,
    invalid_points: HashSet<String>,
    last_values: HashMap<String, Value>,
    rotate_positions: HashMap<String, usize>,
    interval_deadlines: HashMap<String, Instant>,
}

/// Device-layer behavior adapter built from a compiled session.
pub struct BehaviorLayer {
    bindings_by_device: HashMap<String, Vec<ResolvedBehavior>>,
}

impl BehaviorLayer {
    pub fn from_compiled(session: &CompiledModbusSession) -> Option<Self> {
        let active_behavior_set = session.active_behavior_set.as_deref();
        let mut bindings_by_device: HashMap<String, Vec<ResolvedBehavior>> = HashMap::new();

        for binding in &session.compiled_behavior_bindings {
            if binding.behavior_set != "__compat" {
                match active_behavior_set {
                    Some(active) if binding.behavior_set == active => {}
                    _ => continue,
                }
            }

            let actions = binding
                .actions
                .iter()
                .filter_map(|action_name| session.actions.get(action_name).cloned())
                .collect::<Vec<_>>();
            if actions.is_empty() {
                continue;
            }

            bindings_by_device
                .entry(binding.device_id.clone())
                .or_default()
                .push(ResolvedBehavior {
                    name: format!("{}/{}", binding.behavior_set, binding.name),
                    point_id: binding.point_id.clone(),
                    trigger: binding.trigger,
                    condition: binding.condition.clone(),
                    interval_ms: binding.interval_ms,
                    actions,
                });
        }

        if bindings_by_device.is_empty() {
            None
        } else {
            Some(Self { bindings_by_device })
        }
    }
}

impl DevicePortLayer for BehaviorLayer {
    fn decorate(
        &self,
        _protocol: Option<mabi_core::Protocol>,
        port: DynDevicePort,
    ) -> DynDevicePort {
        let device_id = port.id();
        match self.bindings_by_device.get(&device_id) {
            Some(bindings) => Arc::new(BehaviorDevicePort {
                inner: port,
                bindings: bindings.clone(),
                state: Mutex::new(BehaviorState::default()),
            }),
            None => port,
        }
    }
}

struct BehaviorDevicePort {
    inner: DynDevicePort,
    bindings: Vec<ResolvedBehavior>,
    state: Mutex<BehaviorState>,
}

#[async_trait]
impl DevicePort for BehaviorDevicePort {
    fn info(&self) -> DeviceInfo {
        self.inner.info()
    }

    async fn start(&self) -> mabi_core::Result<()> {
        self.inner.start().await
    }

    async fn stop(&self) -> mabi_core::Result<()> {
        self.inner.stop().await
    }

    async fn read(&self, point_id: &str) -> mabi_core::Result<DataPoint> {
        self.apply_startup_and_interval_behaviors().await?;

        let mut point = self.inner.read(point_id).await?;
        let should_apply = self
            .bindings
            .iter()
            .any(|binding| binding.point_id == point_id && binding.trigger == BehaviorTrigger::OnRead);
        if should_apply {
            let final_value = self
                .execute_point_behaviors(point_id, BehaviorTrigger::OnRead, point.value.clone(), false)
                .await?;
            point.value = final_value;
        }

        let invalid = self.state.lock().invalid_points.contains(point_id);
        if invalid {
            point.quality = Quality::BAD;
        }
        self.state
            .lock()
            .last_values
            .insert(point_id.to_string(), point.value.clone());
        Ok(point)
    }

    async fn write(&self, point_id: &str, value: Value) -> mabi_core::Result<()> {
        self.apply_startup_and_interval_behaviors().await?;
        let final_value = self
            .execute_point_behaviors(point_id, BehaviorTrigger::OnWrite, value, true)
            .await?;
        self.inner.write(point_id, final_value.clone()).await?;
        self.state
            .lock()
            .last_values
            .insert(point_id.to_string(), final_value);
        Ok(())
    }

    fn point_definitions(&self) -> Vec<DataPointDef> {
        self.inner.point_definitions()
    }
}

impl BehaviorDevicePort {
    async fn apply_startup_and_interval_behaviors(&self) -> mabi_core::Result<()> {
        let due_behaviors = {
            let mut state = self.state.lock();
            let mut due = Vec::new();
            if !state.startup_applied {
                due.extend(self.bindings.iter().filter(|binding| {
                    matches!(
                        binding.trigger,
                        BehaviorTrigger::OnStartup | BehaviorTrigger::OnReset
                    )
                }).cloned());
                state.startup_applied = true;
            }

            let now = Instant::now();
            for binding in self.bindings.iter().filter(|binding| binding.trigger == BehaviorTrigger::OnInterval) {
                let interval_ms = binding.interval_ms.unwrap_or(1_000);
                let key = format!("{}:{}", binding.name, binding.point_id);
                let due_now = state
                    .interval_deadlines
                    .get(&key)
                    .map(|deadline| *deadline <= now)
                    .unwrap_or(true);
                if due_now {
                    due.push(binding.clone());
                    state
                        .interval_deadlines
                        .insert(key, now + Duration::from_millis(interval_ms));
                }
            }
            due
        };

        for binding in due_behaviors {
            let current = self
                .inner
                .read(&binding.point_id)
                .await
                .map(|point| point.value)
                .unwrap_or(Value::Null);
            let final_value = self
                .execute_resolved_behavior(&binding, current, true)
                .await?;
            self.inner.write(&binding.point_id, final_value.clone()).await?;
            self.state
                .lock()
                .last_values
                .insert(binding.point_id.clone(), final_value);
        }

        Ok(())
    }

    async fn execute_point_behaviors(
        &self,
        point_id: &str,
        trigger: BehaviorTrigger,
        initial_value: Value,
        persist_side_effects: bool,
    ) -> mabi_core::Result<Value> {
        let mut current = initial_value;
        for binding in self
            .bindings
            .iter()
            .filter(|binding| binding.point_id == point_id && binding.trigger == trigger)
        {
            current = self
                .execute_resolved_behavior(binding, current, persist_side_effects)
                .await?;
        }
        Ok(current)
    }

    async fn execute_resolved_behavior(
        &self,
        binding: &ResolvedBehavior,
        current: Value,
        persist_side_effects: bool,
    ) -> mabi_core::Result<Value> {
        let last_value = self
            .state
            .lock()
            .last_values
            .get(&binding.point_id)
            .cloned();
        if !condition_matches(binding.condition.as_ref(), &current, last_value.as_ref()) {
            return Ok(current);
        }

        let (next_value, invalid_delta, side_effects, rotate_updates) =
            plan_actions(binding, current.clone(), &self.state.lock().rotate_positions);

        {
            let mut state = self.state.lock();
            match invalid_delta {
                InvalidDelta::Mark => {
                    state.invalid_points.insert(binding.point_id.clone());
                }
                InvalidDelta::Clear => {
                    state.invalid_points.remove(&binding.point_id);
                }
                InvalidDelta::NoChange => {}
            }
            for (key, value) in rotate_updates {
                state.rotate_positions.insert(key, value);
            }
        }

        if persist_side_effects {
            for (point_id, value) in side_effects {
                self.inner.write(&point_id, value.clone()).await?;
                self.state.lock().last_values.insert(point_id, value);
            }
        }

        Ok(next_value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InvalidDelta {
    NoChange,
    Mark,
    Clear,
}

fn plan_actions(
    binding: &ResolvedBehavior,
    current: Value,
    rotate_positions: &HashMap<String, usize>,
) -> (
    Value,
    InvalidDelta,
    Vec<(String, Value)>,
    BTreeMap<String, usize>,
) {
    let mut current = current;
    let mut invalid_delta = InvalidDelta::NoChange;
    let mut side_effects = Vec::new();
    let mut rotate_updates = BTreeMap::new();

    for action in &binding.actions {
        match action {
            ActionDefinition::SetValue { value } => {
                current = json_to_value(value.clone());
            }
            ActionDefinition::CopyToPoint { target_point_id }
            | ActionDefinition::Mirror { target_point_id } => {
                side_effects.push((target_point_id.clone(), current.clone()));
            }
            ActionDefinition::Scale { factor, offset } => {
                current = apply_numeric(current, |value| value * factor + offset);
            }
            ActionDefinition::Offset { value } => {
                current = apply_numeric(current, |current| current + value);
            }
            ActionDefinition::Clamp { min, max } => {
                current = apply_numeric(current, |value| value.clamp(*min, *max));
            }
            ActionDefinition::Map { mapping, default } => {
                let key = value_key(&current);
                current = mapping
                    .get(&key)
                    .cloned()
                    .or_else(|| default.clone())
                    .map(json_to_value)
                    .unwrap_or(current);
            }
            ActionDefinition::MaskBits { and_mask, or_mask } => {
                if let Some(value) = current.as_i64() {
                    let mut masked = value as u64;
                    if let Some(and_mask) = and_mask {
                        masked &= *and_mask;
                    }
                    if let Some(or_mask) = or_mask {
                        masked |= *or_mask;
                    }
                    current = Value::U64(masked);
                }
            }
            ActionDefinition::MarkInvalid => {
                invalid_delta = InvalidDelta::Mark;
            }
            ActionDefinition::ClearInvalid => {
                invalid_delta = InvalidDelta::Clear;
            }
            ActionDefinition::Rotate { values } => {
                if !values.is_empty() {
                    let key = format!("{}:{}", binding.name, binding.point_id);
                    let next_index = rotate_positions.get(&key).copied().unwrap_or(0) % values.len();
                    current = json_to_value(values[next_index].clone());
                    rotate_updates.insert(key, (next_index + 1) % values.len());
                }
            }
            ActionDefinition::Latch | ActionDefinition::Pulse { .. } => {}
        }
    }

    (current, invalid_delta, side_effects, rotate_updates)
}

fn condition_matches(condition: Option<&BehaviorCondition>, current: &Value, last: Option<&Value>) -> bool {
    let Some(condition) = condition else {
        return true;
    };

    match condition.operator {
        BehaviorConditionOperator::Changed => last.map(|value| value != current).unwrap_or(true),
        BehaviorConditionOperator::Eq => {
            condition.value.as_ref().map(|value| current == &json_to_value(value.clone())).unwrap_or(false)
        }
        BehaviorConditionOperator::Ne => {
            condition.value.as_ref().map(|value| current != &json_to_value(value.clone())).unwrap_or(false)
        }
        BehaviorConditionOperator::Gt => compare_numeric(current, condition.value.as_ref(), |left, right| left > right),
        BehaviorConditionOperator::Gte => compare_numeric(current, condition.value.as_ref(), |left, right| left >= right),
        BehaviorConditionOperator::Lt => compare_numeric(current, condition.value.as_ref(), |left, right| left < right),
        BehaviorConditionOperator::Lte => compare_numeric(current, condition.value.as_ref(), |left, right| left <= right),
    }
}

fn compare_numeric(
    current: &Value,
    expected: Option<&serde_json::Value>,
    predicate: impl FnOnce(f64, f64) -> bool,
) -> bool {
    let Some(left) = current.as_f64() else {
        return false;
    };
    let Some(expected) = expected else {
        return false;
    };
    let right = json_to_value(expected.clone()).as_f64();
    right.map(|right| predicate(left, right)).unwrap_or(false)
}

fn apply_numeric(value: Value, transform: impl FnOnce(f64) -> f64) -> Value {
    match value {
        Value::F64(_) | Value::F32(_) => {
            let numeric = value.as_f64().unwrap_or_default();
            Value::F64(transform(numeric))
        }
        _ => value
            .as_i64()
            .map(|numeric| Value::I64(transform(numeric as f64).round() as i64))
            .unwrap_or(value),
    }
}

fn json_to_value(value: serde_json::Value) -> Value {
    serde_json::from_value(value.clone()).unwrap_or_else(|_| match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Bool(value),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Value::I64(value)
            } else if let Some(value) = number.as_u64() {
                Value::U64(value)
            } else {
                Value::F64(number.as_f64().unwrap_or_default())
            }
        }
        serde_json::Value::String(value) => Value::String(value),
        serde_json::Value::Array(values) => {
            Value::Array(values.into_iter().map(json_to_value).collect())
        }
        serde_json::Value::Object(_) => Value::String(value.to_string()),
    })
}

fn value_key(value: &Value) -> String {
    match value {
        Value::Bool(value) => value.to_string(),
        Value::String(value) => value.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| value.type_name().to_string()),
    }
}
