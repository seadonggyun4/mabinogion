//! Session-centric simulator configuration and DX-oriented inspection helpers.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

use mabi_runtime::{ProtocolLaunchSpec, RuntimeExtensions};

use crate::behavior::BehaviorLayer;
use crate::error::{ModbusError, ModbusResult};
use crate::fault_injection::{
    ExtraDataMode, FaultConfig, FaultInjectionConfig, FaultTarget, FaultTypeConfig,
    PartialFrameMode, TruncationMode,
};
use crate::profile::{DatastoreKind, GeneratedProfilePreset, SimulatorProfile};
use crate::rtu::{PerformancePreset as RtuPerformancePreset, RtuServerConfig};
use crate::tcp::PerformancePreset as TcpPerformancePreset;
use mabi_core::types::{DataType, ModbusRegisterType};

/// Canonical session-centric config surface for the Modbus simulator.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModbusSimulatorConfig {
    #[serde(default)]
    pub defaults: SimulatorDefaults,
    #[serde(default)]
    pub transports: BTreeMap<String, TransportDefinition>,
    #[serde(default)]
    pub datastores: BTreeMap<String, DatastoreDefinition>,
    #[serde(default)]
    pub devices: BTreeMap<String, DeviceBundleDefinition>,
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionDefinition>,
    #[serde(default)]
    pub presets: BTreeMap<String, GeneratedPresetDefinition>,
    #[serde(default)]
    pub actions: BTreeMap<String, ActionDefinition>,
    #[serde(default)]
    pub behaviors: BTreeMap<String, BehaviorDefinition>,
    #[serde(default)]
    pub response_profiles: BTreeMap<String, ResponseProfileDefinition>,
}

impl ModbusSimulatorConfig {
    /// Loads a simulator config from a YAML, JSON, or TOML file.
    pub fn from_path(path: &Path) -> ModbusResult<Self> {
        let content = std::fs::read_to_string(path)?;
        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
        Self::from_str_with_format(&content, extension)
    }

    /// Parses a config from the supplied content and file-format hint.
    pub fn from_str_with_format(content: &str, format: &str) -> ModbusResult<Self> {
        let parsed: Self = match format {
            "yaml" | "yml" => serde_yaml::from_str(content)
                .map_err(|error| ModbusError::Config(format!("invalid YAML config: {}", error)))?,
            "json" => serde_json::from_str(content)
                .map_err(|error| ModbusError::Config(format!("invalid JSON config: {}", error)))?,
            "toml" => toml::from_str(content)
                .map_err(|error| ModbusError::Config(format!("invalid TOML config: {}", error)))?,
            other => {
                return Err(ModbusError::Config(format!(
                    "unsupported config format: {}",
                    other
                )))
            }
        };
        parsed.validate()?;
        Ok(parsed)
    }

    /// Validates references and compile-time invariants across the config.
    pub fn validate(&self) -> ModbusResult<()> {
        if self.sessions.is_empty() {
            return Err(ModbusError::Config(
                "simulator config must define at least one session".into(),
            ));
        }

        for (name, session) in &self.sessions {
            if !self.transports.contains_key(&session.transport) {
                return Err(ModbusError::Config(format!(
                    "session '{}' references unknown transport '{}'",
                    name, session.transport
                )));
            }
            if session.devices.is_empty() && session.preset.is_none() {
                return Err(ModbusError::Config(format!(
                    "session '{}' must reference at least one device bundle or preset",
                    name
                )));
            }
            for device in &session.devices {
                if !self.devices.contains_key(device) {
                    return Err(ModbusError::Config(format!(
                        "session '{}' references unknown device bundle '{}'",
                        name, device
                    )));
                }
            }
            if let Some(preset) = &session.preset {
                if !self.presets.contains_key(preset) {
                    return Err(ModbusError::Config(format!(
                        "session '{}' references unknown preset '{}'",
                        name, preset
                    )));
                }
            }
            if let Some(active) = &session.active_fault_preset {
                if !session.fault_presets.contains_key(active) {
                    return Err(ModbusError::Config(format!(
                        "session '{}' references unknown fault preset '{}'",
                        name, active
                    )));
                }
            }
            if let Some(active) = &session.active_response_profile {
                if !self.response_profiles.contains_key(active) {
                    return Err(ModbusError::Config(format!(
                        "session '{}' references unknown response profile '{}'",
                        name, active
                    )));
                }
            }
            if let Some(active) = &session.active_behavior_set {
                if !session.behavior_sets.contains_key(active) {
                    return Err(ModbusError::Config(format!(
                        "session '{}' references unknown behavior set '{}'",
                        name, active
                    )));
                }
            }

            let mut unit_ids = BTreeSet::new();
            let compiled_profile = self.compile_profile(session)?;
            for unit in &compiled_profile.units {
                if !unit_ids.insert(unit.unit_id) {
                    return Err(ModbusError::Config(format!(
                        "session '{}' contains duplicate unit id {}",
                        name, unit.unit_id
                    )));
                }

                let mut point_ids = BTreeSet::new();
                for point in &unit.points {
                    if !point_ids.insert(point.id.clone()) {
                        return Err(ModbusError::Config(format!(
                            "session '{}' contains duplicate point id '{}' in unit {}",
                            name, point.id, unit.unit_id
                        )));
                    }
                }
            }

            for device_name in &session.devices {
                let bundle = self.devices.get(device_name).ok_or_else(|| {
                    ModbusError::Config(format!("unknown device bundle '{}'", device_name))
                })?;
                for unit in &bundle.units {
                    let point_ids = unit
                        .points
                        .iter()
                        .map(|point| point.id.as_str())
                        .collect::<BTreeSet<_>>();
                    for binding in &unit.action_bindings {
                        if !point_ids.contains(binding.point_id.as_str()) {
                            return Err(ModbusError::Config(format!(
                                "unit {} references unknown point '{}' in action bindings",
                                unit.unit_id, binding.point_id
                            )));
                        }
                        for action in &binding.bindings {
                            if !self.actions.contains_key(&action.action) {
                                return Err(ModbusError::Config(format!(
                                    "unit {} references unknown action '{}'",
                                    unit.unit_id, action.action
                                )));
                            }
                        }
                    }
                }
            }

            for (set_name, behavior_set) in &session.behavior_sets {
                for behavior_name in &behavior_set.behaviors {
                    let behavior = self.behaviors.get(behavior_name).ok_or_else(|| {
                        ModbusError::Config(format!(
                            "session '{}' behavior set '{}' references unknown behavior '{}'",
                            name, set_name, behavior_name
                        ))
                    })?;
                    for action_name in &behavior.actions {
                        if !self.actions.contains_key(action_name) {
                            return Err(ModbusError::Config(format!(
                                "behavior '{}' references unknown action '{}'",
                                behavior_name, action_name
                            )));
                        }
                    }

                    let matches = matching_behavior_targets(behavior, &compiled_profile);
                    if matches.is_empty() {
                        return Err(ModbusError::Config(format!(
                            "behavior '{}' does not match any point in session '{}'",
                            behavior_name, name
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    /// Compiles a named session into a runtime launch spec and DX metadata.
    pub fn compile_session(&self, name: &str) -> ModbusResult<CompiledModbusSession> {
        let session = self
            .sessions
            .get(name)
            .ok_or_else(|| ModbusError::Config(format!("unknown session '{}'", name)))?;
        let profile = self.compile_profile(session)?;
        let transport = self.transports.get(&session.transport).ok_or_else(|| {
            ModbusError::Config(format!(
                "session '{}' references unknown transport '{}'",
                name, session.transport
            ))
        })?;

        let launch = ProtocolLaunchSpec {
            protocol: "modbus".into(),
            name: Some(
                session
                    .service_name
                    .clone()
                    .unwrap_or_else(|| name.to_string()),
            ),
            config: serde_json::to_value(ModbusServiceLaunchConfig::from_session(
                &self.defaults,
                transport,
                profile.clone(),
            ))
            .map_err(|error| {
                ModbusError::Config(format!("failed to encode session launch config: {}", error))
            })?,
        };

        let metadata = self.compile_session_metadata(session, &profile)?;

        Ok(CompiledModbusSession {
            session_name: name.to_string(),
            launch,
            transport_kind: transport.kind(),
            profile,
            trace: session.trace.resolved(&self.defaults),
            reset: session.reset.clone(),
            control: session.control.clone(),
            fault_presets: session.fault_presets.clone(),
            active_fault_preset: session.active_fault_preset.clone(),
            response_profiles: self.response_profiles.clone(),
            active_response_profile: session.active_response_profile.clone(),
            actions: self.actions.clone(),
            behaviors: self.behaviors.clone(),
            behavior_sets: session.behavior_sets.clone(),
            active_behavior_set: session.active_behavior_set.clone(),
            point_catalog: metadata.point_catalog,
            datastore_policies: metadata.datastore_policies,
            action_binding_summaries: metadata.action_bindings,
            behavior_binding_summaries: metadata.behavior_bindings,
            compiled_behavior_bindings: metadata.compiled_behavior_bindings,
            readiness_timeout_ms: session
                .readiness_timeout_ms
                .or(self.defaults.readiness_timeout_ms),
        })
    }

    /// Returns a human-oriented summary for inspection surfaces.
    pub fn inspect_summary(&self) -> ModbusConfigSummary {
        ModbusConfigSummary {
            transports: self.transports.keys().cloned().collect(),
            datastores: self.datastores.keys().cloned().collect(),
            devices: self.devices.keys().cloned().collect(),
            sessions: self
                .sessions
                .iter()
                .map(|(name, session)| SessionSummary {
                    name: name.clone(),
                    transport: session.transport.clone(),
                    devices: session.devices.clone(),
                    preset: session.preset.clone(),
                    active_fault_preset: session.active_fault_preset.clone(),
                    active_response_profile: session.active_response_profile.clone(),
                    active_behavior_set: session.active_behavior_set.clone(),
                })
                .collect(),
            presets: self.presets.keys().cloned().collect(),
            actions: self.actions.keys().cloned().collect(),
            behaviors: self.behaviors.keys().cloned().collect(),
            response_profiles: self.response_profiles.keys().cloned().collect(),
        }
    }

    fn compile_profile(&self, session: &SessionDefinition) -> ModbusResult<SimulatorProfile> {
        let mut profile = if let Some(preset_name) = &session.preset {
            self.presets
                .get(preset_name)
                .ok_or_else(|| ModbusError::Config(format!("unknown preset '{}'", preset_name)))?
                .build(&self.datastores)?
        } else {
            SimulatorProfile::new()
        };

        for device_name in &session.devices {
            let bundle = self.devices.get(device_name).ok_or_else(|| {
                ModbusError::Config(format!("unknown device bundle '{}'", device_name))
            })?;
            let compiled = bundle.compile(&self.datastores)?;
            profile.broadcast_enabled |= compiled.broadcast_enabled;
            profile.units.extend(compiled.units);
        }

        Ok(profile)
    }

    fn compile_session_metadata(
        &self,
        session: &SessionDefinition,
        profile: &SimulatorProfile,
    ) -> ModbusResult<CompiledSessionMetadata> {
        let mut point_catalog = BTreeMap::new();
        for unit in &profile.units {
            let device_id = format!("modbus-{}", unit.unit_id);
            for point in &unit.points {
                point_catalog.insert(
                    point_catalog_key(&device_id, &point.id),
                    CompiledPointMetadata {
                        device_id: device_id.clone(),
                        point_id: point.id.clone(),
                        source_datastore: None,
                        read_only: matches!(
                            point.register_type,
                            ModbusRegisterType::InputRegister | ModbusRegisterType::DiscreteInput
                        ),
                        invalid: false,
                        action_bindings: Vec::new(),
                        behavior_bindings: Vec::new(),
                    },
                );
            }
        }

        let mut datastore_policies = BTreeMap::new();
        let mut action_bindings = Vec::new();
        let mut behavior_bindings = Vec::new();
        let mut compiled_behavior_bindings = Vec::new();

        if let Some(preset_name) = &session.preset {
            if let Some(datastore) = self
                .presets
                .get(preset_name)
                .and_then(|preset| preset.datastore.as_ref())
            {
                let datastore_name = datastore.reference_name();
                let resolved = datastore.resolve(&self.datastores)?;
                datastore_policies.insert(
                    datastore_name.clone().unwrap_or_else(|| format!("preset:{}", preset_name)),
                    resolved.policy_summary(datastore_name.as_deref()),
                );

                for unit in &profile.units {
                    let device_id = format!("modbus-{}", unit.unit_id);
                    for point in &unit.points {
                        let key = point_catalog_key(&device_id, &point.id);
                        if let Some(entry) = point_catalog.get_mut(&key) {
                            entry.source_datastore = datastore_name.clone();
                            entry.read_only =
                                entry.read_only || resolved.is_read_only(point.register_type, point.address);
                            entry.invalid = resolved.is_invalid(point.register_type, point.address);
                        }
                    }
                }
            }
        }

        for device_name in &session.devices {
            let bundle = self.devices.get(device_name).ok_or_else(|| {
                ModbusError::Config(format!("unknown device bundle '{}'", device_name))
            })?;
            for unit in &bundle.units {
                let device_id = format!("modbus-{}", unit.unit_id);
                let datastore_name = unit
                    .datastore
                    .as_ref()
                    .and_then(DatastoreSelector::reference_name);
                let datastore = match &unit.datastore {
                    Some(selector) => selector.resolve(&self.datastores)?,
                    None => DatastoreDefinition::default(),
                };
                let summary_name = datastore_name
                    .clone()
                    .unwrap_or_else(|| format!("{}/unit-{}", device_name, unit.unit_id));
                datastore_policies.insert(summary_name, datastore.policy_summary(datastore_name.as_deref()));

                let binding_map = unit.binding_summary_map();
                let binding_defs = unit.binding_definition_map();
                for point in &unit.points {
                    let key = point_catalog_key(&device_id, &point.id);
                    let actions = binding_map.get(&point.id).cloned().unwrap_or_default();
                    if let Some(entry) = point_catalog.get_mut(&key) {
                        entry.source_datastore = datastore_name.clone();
                        entry.read_only =
                            entry.read_only || datastore.is_read_only(point.register_type, point.address);
                        entry.invalid = datastore.is_invalid(point.register_type, point.address);
                        entry.action_bindings = actions.clone();
                        entry.behavior_bindings = actions.clone();
                    }
                    if !actions.is_empty() {
                        action_bindings.push(ActionBindingSummary {
                            device_id: device_id.clone(),
                            point_id: point.id.clone(),
                            bindings: actions,
                        });
                    }
                    if let Some(definitions) = binding_defs.get(&point.id) {
                        for definition in definitions {
                            compiled_behavior_bindings.push(CompiledBehaviorBinding {
                                name: definition.action.clone(),
                                behavior_set: "__compat".to_string(),
                                device_id: device_id.clone(),
                                point_id: point.id.clone(),
                                trigger: definition.trigger,
                                condition: None,
                                interval_ms: None,
                                actions: vec![definition.action.clone()],
                            });
                        }
                    }
                }
            }
        }

        for (set_name, behavior_set) in &session.behavior_sets {
            for behavior_name in &behavior_set.behaviors {
                let behavior = self.behaviors.get(behavior_name).ok_or_else(|| {
                    ModbusError::Config(format!("unknown behavior '{}'", behavior_name))
                })?;
                for target in matching_behavior_targets(behavior, profile) {
                    let key = point_catalog_key(&target.device_id, &target.point_id);
                    if let Some(entry) = point_catalog.get_mut(&key) {
                        entry.behavior_bindings.push(format!("{}@{}", behavior_name, set_name));
                    }
                    behavior_bindings.push(BehaviorBindingSummary {
                        device_id: target.device_id.clone(),
                        point_id: target.point_id.clone(),
                        behavior: behavior_name.clone(),
                        behavior_set: set_name.clone(),
                        trigger: behavior.trigger,
                    });
                    compiled_behavior_bindings.push(CompiledBehaviorBinding {
                        name: behavior_name.clone(),
                        behavior_set: set_name.clone(),
                        device_id: target.device_id,
                        point_id: target.point_id,
                        trigger: behavior.trigger,
                        condition: behavior.condition.clone(),
                        interval_ms: behavior.interval_ms,
                        actions: behavior.actions.clone(),
                    });
                }
            }
        }

        Ok(CompiledSessionMetadata {
            point_catalog,
            datastore_policies: datastore_policies.into_values().collect(),
            action_bindings,
            behavior_bindings,
            compiled_behavior_bindings,
        })
    }
}

/// Defaults applied during session compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatorDefaults {
    #[serde(default = "default_trace_capacity")]
    pub trace_capacity: usize,
    #[serde(default)]
    pub tcp_performance_preset: TcpPerformancePreset,
    #[serde(default)]
    pub rtu_performance_preset: RtuPerformancePreset,
    #[serde(default)]
    pub readiness_timeout_ms: Option<u64>,
}

impl Default for SimulatorDefaults {
    fn default() -> Self {
        Self {
            trace_capacity: default_trace_capacity(),
            tcp_performance_preset: TcpPerformancePreset::Default,
            rtu_performance_preset: RtuPerformancePreset::Default,
            readiness_timeout_ms: None,
        }
    }
}

fn default_trace_capacity() -> usize {
    256
}

/// Named transport definition, inspired by PyModbus `server_list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransportDefinition {
    Tcp {
        #[serde(default = "default_bind")]
        bind: String,
        #[serde(default = "default_tcp_port")]
        port: u16,
        #[serde(default)]
        performance_preset: Option<TcpPerformancePreset>,
    },
    Rtu {
        #[serde(default)]
        config: RtuServerConfig,
    },
}

impl TransportDefinition {
    fn kind(&self) -> CompiledTransportKind {
        match self {
            Self::Tcp { .. } => CompiledTransportKind::Tcp,
            Self::Rtu { .. } => CompiledTransportKind::Rtu,
        }
    }
}

fn default_bind() -> String {
    "0.0.0.0".to_string()
}

fn default_tcp_port() -> u16 {
    502
}

/// Address-range policy used by named datastore definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatastoreAddressRange {
    pub register_type: ModbusRegisterType,
    pub start: u16,
    pub quantity: u16,
}

impl DatastoreAddressRange {
    fn matches(&self, register_type: ModbusRegisterType, address: u16) -> bool {
        self.register_type == register_type
            && address >= self.start
            && address < self.start.saturating_add(self.quantity)
    }
}

/// Named reusable typed block metadata for simulator datastores.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatastoreTypedBlock {
    pub register_type: ModbusRegisterType,
    pub start: u16,
    pub quantity: u16,
    pub data_type: DataType,
}

/// Optional repeat metadata for patterned simulators.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatastoreRepeatPolicy {
    pub every: u16,
    pub count: u16,
}

/// Initialization hints compiled alongside a datastore definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatastoreInitialization {
    Zero,
    One,
    Max,
    Preserve,
}

/// Reusable named datastore definitions for simulator device bundles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatastoreDefinition {
    #[serde(flatten)]
    pub kind: DatastoreKind,
    #[serde(default)]
    pub invalid_ranges: Vec<DatastoreAddressRange>,
    #[serde(default)]
    pub readonly_ranges: Vec<DatastoreAddressRange>,
    #[serde(default)]
    pub typed_blocks: Vec<DatastoreTypedBlock>,
    #[serde(default)]
    pub default_value: Option<JsonValue>,
    #[serde(default)]
    pub repeat: Option<DatastoreRepeatPolicy>,
    #[serde(default)]
    pub initialization: Option<DatastoreInitialization>,
}

impl Default for DatastoreDefinition {
    fn default() -> Self {
        Self {
            kind: DatastoreKind::default(),
            invalid_ranges: Vec::new(),
            readonly_ranges: Vec::new(),
            typed_blocks: Vec::new(),
            default_value: None,
            repeat: None,
            initialization: None,
        }
    }
}

impl From<DatastoreKind> for DatastoreDefinition {
    fn from(value: DatastoreKind) -> Self {
        Self {
            kind: value,
            ..Default::default()
        }
    }
}

impl DatastoreDefinition {
    fn is_invalid(&self, register_type: ModbusRegisterType, address: u16) -> bool {
        self.invalid_ranges
            .iter()
            .any(|range| range.matches(register_type, address))
    }

    fn is_read_only(&self, register_type: ModbusRegisterType, address: u16) -> bool {
        matches!(
            register_type,
            ModbusRegisterType::InputRegister | ModbusRegisterType::DiscreteInput
        ) || self
            .readonly_ranges
            .iter()
            .any(|range| range.matches(register_type, address))
    }

    fn policy_summary(&self, name: Option<&str>) -> DatastorePolicySummary {
        DatastorePolicySummary {
            name: name.unwrap_or("inline").to_string(),
            kind: match self.kind {
                DatastoreKind::Dense { .. } => "dense".to_string(),
                DatastoreKind::Sparse { .. } => "sparse".to_string(),
            },
            invalid_ranges: self.invalid_ranges.len(),
            readonly_ranges: self.readonly_ranges.len(),
            typed_blocks: self.typed_blocks.len(),
            has_default_value: self.default_value.is_some(),
            repeat: self.repeat.clone(),
            initialization: self
                .initialization
                .as_ref()
                .map(|value| format!("{:?}", value).to_lowercase()),
        }
    }
}

/// Named or inline datastore selection used by devices and presets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DatastoreSelector {
    Named(String),
    Inline(DatastoreDefinition),
}

impl DatastoreSelector {
    fn resolve(
        &self,
        datastores: &BTreeMap<String, DatastoreDefinition>,
    ) -> ModbusResult<DatastoreDefinition> {
        match self {
            Self::Named(name) => datastores
                .get(name)
                .cloned()
                .ok_or_else(|| ModbusError::Config(format!("unknown datastore '{}'", name))),
            Self::Inline(datastore) => Ok(datastore.clone()),
        }
    }

    fn reference_name(&self) -> Option<String> {
        match self {
            Self::Named(name) => Some(name.clone()),
            Self::Inline(_) => None,
        }
    }
}

/// Target selector for deterministic behaviors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorTarget {
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub unit_id: Option<u8>,
    pub point_id: String,
}

/// Supported deterministic comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorConditionOperator {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    Changed,
}

/// Optional deterministic guard used by a behavior definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorCondition {
    pub operator: BehaviorConditionOperator,
    #[serde(default)]
    pub value: Option<JsonValue>,
}

/// Deterministic behavior triggers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorTrigger {
    OnRead,
    #[default]
    OnWrite,
    OnInterval,
    #[serde(alias = "on_boot")]
    OnStartup,
    OnReset,
}

impl BehaviorTrigger {
    fn as_str(self) -> &'static str {
        match self {
            Self::OnRead => "on_read",
            Self::OnWrite => "on_write",
            Self::OnInterval => "on_interval",
            Self::OnStartup => "on_startup",
            Self::OnReset => "on_reset",
        }
    }
}

/// Backward-compatible alias for older action binding configs.
pub type ActionTrigger = BehaviorTrigger;

/// Deterministic action catalog definition inspired by PyModbus simulator hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionDefinition {
    SetValue { value: JsonValue },
    CopyToPoint { target_point_id: String },
    Clamp { min: f64, max: f64 },
    Mirror { target_point_id: String },
    Scale {
        factor: f64,
        #[serde(default)]
        offset: f64,
    },
    Offset { value: f64 },
    Map {
        mapping: BTreeMap<String, JsonValue>,
        #[serde(default)]
        default: Option<JsonValue>,
    },
    MaskBits {
        #[serde(default)]
        and_mask: Option<u64>,
        #[serde(default)]
        or_mask: Option<u64>,
    },
    MarkInvalid,
    ClearInvalid,
    Latch,
    Pulse { duration_ms: u64 },
    Rotate { values: Vec<JsonValue> },
}

/// Deterministic behavior definition referencing a catalog of actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorDefinition {
    pub target: BehaviorTarget,
    #[serde(default)]
    pub trigger: BehaviorTrigger,
    #[serde(default)]
    pub condition: Option<BehaviorCondition>,
    #[serde(default)]
    pub interval_ms: Option<u64>,
    #[serde(default)]
    pub actions: Vec<String>,
}

/// Named behavior set scoped to a session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BehaviorSetDefinition {
    #[serde(default)]
    pub behaviors: Vec<String>,
}

/// Action binding applied to a point definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionBindingDefinition {
    pub action: String,
    #[serde(default)]
    pub trigger: BehaviorTrigger,
}

impl ActionBindingDefinition {
    fn summary(&self) -> String {
        format!("{}@{}", self.action, self.trigger.as_str())
    }
}

/// Point-level action bindings compiled into session metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PointActionBinding {
    pub point_id: String,
    #[serde(default)]
    pub bindings: Vec<ActionBindingDefinition>,
}

/// Deterministic wire-level response profile.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResponseProfileDefinition {
    #[serde(default)]
    pub delay_ms: Option<u64>,
    #[serde(default)]
    pub exception_code: Option<u8>,
    #[serde(default)]
    pub split_response: Option<SplitResponseDefinition>,
    #[serde(default)]
    pub partial_response: Option<PartialResponseDefinition>,
    #[serde(default)]
    pub silent_drop: bool,
    #[serde(default)]
    pub malformed_response: Option<MalformedResponseDefinition>,
}

impl ResponseProfileDefinition {
    fn to_fault_injection_config(
        &self,
        transport_kind: CompiledTransportKind,
    ) -> Option<FaultInjectionConfig> {
        let mut config = FaultInjectionConfig::new();

        if let Some(delay_ms) = self.delay_ms {
            config = config.with_fault(FaultConfig::delayed_response(
                std::time::Duration::from_millis(delay_ms),
                std::time::Duration::ZERO,
                FaultTarget::new(),
            ));
        }
        if let Some(exception_code) = self.exception_code {
            config = config.with_fault(FaultConfig::exception_injection(
                exception_code,
                FaultTarget::new(),
            ));
        }
        if self.silent_drop {
            config = config.with_fault(FaultConfig::no_response(FaultTarget::new()));
        }
        if let Some(partial) = &self.partial_response {
            config = config.with_fault(partial.to_fault_config(transport_kind));
        }
        if let Some(split) = &self.split_response {
            config = config.with_fault(split.to_fault_config(transport_kind));
        }
        if let Some(malformed) = &self.malformed_response {
            malformed.append_faults(transport_kind, &mut config);
        }

        if config.faults.is_empty() {
            None
        } else {
            Some(config)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitResponseDefinition {
    pub first_chunk_bytes: usize,
}

impl SplitResponseDefinition {
    fn to_fault_config(&self, transport_kind: CompiledTransportKind) -> FaultConfig {
        match transport_kind {
            CompiledTransportKind::Tcp => FaultConfig {
                fault_type: crate::fault_injection::FaultType::TruncatedResponse,
                target: FaultTarget::new(),
                config: FaultTypeConfig {
                    truncation_mode: Some(TruncationMode::FixedBytes),
                    truncation_bytes: Some(self.first_chunk_bytes),
                    ..Default::default()
                },
            },
            CompiledTransportKind::Rtu => FaultConfig {
                fault_type: crate::fault_injection::FaultType::PartialFrame,
                target: FaultTarget::new(),
                config: FaultTypeConfig {
                    partial_mode: Some(PartialFrameMode::FixedCount),
                    partial_bytes: Some(self.first_chunk_bytes),
                    ..Default::default()
                },
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialResponseDefinition {
    #[serde(default)]
    pub bytes: Option<usize>,
    #[serde(default)]
    pub percentage: Option<f64>,
}

impl PartialResponseDefinition {
    fn to_fault_config(&self, transport_kind: CompiledTransportKind) -> FaultConfig {
        match transport_kind {
            CompiledTransportKind::Tcp => FaultConfig {
                fault_type: crate::fault_injection::FaultType::TruncatedResponse,
                target: FaultTarget::new(),
                config: FaultTypeConfig {
                    truncation_mode: Some(if self.bytes.is_some() {
                        TruncationMode::FixedBytes
                    } else {
                        TruncationMode::Percentage
                    }),
                    truncation_bytes: self.bytes,
                    truncation_percentage: self.percentage,
                    ..Default::default()
                },
            },
            CompiledTransportKind::Rtu => FaultConfig {
                fault_type: crate::fault_injection::FaultType::PartialFrame,
                target: FaultTarget::new(),
                config: FaultTypeConfig {
                    partial_mode: Some(if self.bytes.is_some() {
                        PartialFrameMode::FixedCount
                    } else {
                        PartialFrameMode::Percentage
                    }),
                    partial_bytes: self.bytes,
                    partial_percentage: self.percentage,
                    ..Default::default()
                },
            },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MalformedResponseDefinition {
    #[serde(default)]
    pub truncate_bytes: Option<usize>,
    #[serde(default)]
    pub append_bytes: Option<Vec<u8>>,
    #[serde(default)]
    pub append_random_count: Option<usize>,
    #[serde(default)]
    pub header_only: bool,
}

impl MalformedResponseDefinition {
    fn append_faults(
        &self,
        _transport_kind: CompiledTransportKind,
        config: &mut FaultInjectionConfig,
    ) {
        if self.header_only {
            config.faults.push(FaultConfig {
                fault_type: crate::fault_injection::FaultType::TruncatedResponse,
                target: FaultTarget::new(),
                config: FaultTypeConfig {
                    truncation_mode: Some(TruncationMode::HeaderOnly),
                    ..Default::default()
                },
            });
        }

        if let Some(bytes) = self.truncate_bytes {
            config.faults.push(FaultConfig {
                fault_type: crate::fault_injection::FaultType::TruncatedResponse,
                target: FaultTarget::new(),
                config: FaultTypeConfig {
                    truncation_mode: Some(TruncationMode::RemoveLastN),
                    truncation_bytes: Some(bytes),
                    ..Default::default()
                },
            });
        }

        if let Some(bytes) = &self.append_bytes {
            config.faults.push(FaultConfig {
                fault_type: crate::fault_injection::FaultType::ExtraData,
                target: FaultTarget::new(),
                config: FaultTypeConfig {
                    extra_data_mode: Some(ExtraDataMode::AppendBytes),
                    extra_bytes: Some(bytes.clone()),
                    extra_count: Some(bytes.len()),
                    ..Default::default()
                },
            });
        }

        if let Some(count) = self.append_random_count {
            config.faults
                .push(FaultConfig::extra_data(ExtraDataMode::AppendRandom, count, FaultTarget::new()));
        }
    }
}

/// Richer simulator device bundle definition inspired by PyModbus contexts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceBundleDefinition {
    #[serde(default = "default_true")]
    pub broadcast_enabled: bool,
    #[serde(default)]
    pub units: Vec<UnitDefinition>,
}

impl DeviceBundleDefinition {
    fn compile(
        &self,
        datastores: &BTreeMap<String, DatastoreDefinition>,
    ) -> ModbusResult<SimulatorProfile> {
        let mut profile = SimulatorProfile::new();
        profile.broadcast_enabled = self.broadcast_enabled;
        for unit in &self.units {
            profile.units.push(unit.compile(datastores)?);
        }
        Ok(profile)
    }
}

/// File-backed unit definition with reusable datastore references.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitDefinition {
    pub unit_id: u8,
    pub name: String,
    #[serde(default)]
    pub datastore: Option<DatastoreSelector>,
    #[serde(default)]
    pub points: Vec<crate::profile::PointProfile>,
    #[serde(default)]
    pub response_delay_ms: u64,
    #[serde(default)]
    pub word_order: crate::types::WordOrder,
    #[serde(default = "default_true")]
    pub broadcast_enabled: bool,
    #[serde(default)]
    pub action_bindings: Vec<PointActionBinding>,
    #[serde(default, skip_serializing_if = "mabi_core::tags::Tags::is_empty")]
    pub tags: mabi_core::tags::Tags,
}

impl UnitDefinition {
    fn compile(
        &self,
        datastores: &BTreeMap<String, DatastoreDefinition>,
    ) -> ModbusResult<crate::profile::UnitProfile> {
        let datastore = match &self.datastore {
            Some(datastore) => datastore.resolve(datastores)?.kind,
            None => DatastoreKind::default(),
        };

        Ok(crate::profile::UnitProfile {
            unit_id: self.unit_id,
            name: self.name.clone(),
            datastore,
            points: self.points.clone(),
            response_delay_ms: self.response_delay_ms,
            word_order: self.word_order,
            broadcast_enabled: self.broadcast_enabled,
            tags: self.tags.clone(),
        })
    }

    fn binding_summary_map(&self) -> BTreeMap<String, Vec<String>> {
        self.action_bindings
            .iter()
            .map(|binding| {
                (
                    binding.point_id.clone(),
                    binding.bindings.iter().map(ActionBindingDefinition::summary).collect(),
                )
            })
            .collect()
    }

    fn binding_definition_map(&self) -> BTreeMap<String, Vec<ActionBindingDefinition>> {
        self.action_bindings
            .iter()
            .map(|binding| (binding.point_id.clone(), binding.bindings.clone()))
            .collect()
    }
}

/// Generated quickstart preset replacing the old numeric CLI surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedPresetDefinition {
    pub devices: usize,
    pub points_per_device: usize,
    #[serde(default)]
    pub datastore: Option<DatastoreSelector>,
}

impl GeneratedPresetDefinition {
    fn build(
        &self,
        datastores: &BTreeMap<String, DatastoreDefinition>,
    ) -> ModbusResult<SimulatorProfile> {
        let mut profile = GeneratedProfilePreset::new(self.devices, self.points_per_device).build();
        if let Some(datastore) = &self.datastore {
            let datastore = datastore.resolve(datastores)?.kind;
            for unit in &mut profile.units {
                unit.datastore = datastore.clone();
            }
        }
        Ok(profile)
    }
}

/// Session-level DX configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDefinition {
    pub transport: String,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub trace: SessionTraceConfig,
    #[serde(default)]
    pub reset: SessionResetPolicy,
    #[serde(default)]
    pub fault_presets: BTreeMap<String, FaultInjectionConfig>,
    #[serde(default)]
    pub active_fault_preset: Option<String>,
    #[serde(default)]
    pub active_response_profile: Option<String>,
    #[serde(default)]
    pub behavior_sets: BTreeMap<String, BehaviorSetDefinition>,
    #[serde(default)]
    pub active_behavior_set: Option<String>,
    #[serde(default)]
    pub readiness_timeout_ms: Option<u64>,
    #[serde(default)]
    pub control: SessionControlConfig,
}

/// Trace buffer configuration for operator-facing control flows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTraceConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub capacity: Option<usize>,
}

impl Default for SessionTraceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            capacity: None,
        }
    }
}

impl SessionTraceConfig {
    fn resolved(&self, defaults: &SimulatorDefaults) -> Self {
        Self {
            enabled: self.enabled,
            capacity: Some(self.capacity.unwrap_or(defaults.trace_capacity)),
        }
    }

    pub fn buffer_capacity(&self) -> usize {
        self.capacity.unwrap_or(default_trace_capacity())
    }
}

fn default_true() -> bool {
    true
}

/// Session reset defaults for the control-plane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResetPolicy {
    #[serde(default = "default_true")]
    pub clear_fault_preset: bool,
    #[serde(default = "default_true")]
    pub clear_response_profile: bool,
    #[serde(default = "default_true")]
    pub clear_behavior_set: bool,
    #[serde(default = "default_true")]
    pub clear_trace_buffer: bool,
}

impl Default for SessionResetPolicy {
    fn default() -> Self {
        Self {
            clear_fault_preset: true,
            clear_response_profile: true,
            clear_behavior_set: true,
            clear_trace_buffer: true,
        }
    }
}

/// Default CLI/control hints compiled alongside a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionControlConfig {
    #[serde(default = "default_trace_tail")]
    pub default_trace_tail: usize,
    #[serde(default = "default_point_limit")]
    pub default_point_limit: usize,
}

impl Default for SessionControlConfig {
    fn default() -> Self {
        Self {
            default_trace_tail: default_trace_tail(),
            default_point_limit: default_point_limit(),
        }
    }
}

fn default_trace_tail() -> usize {
    20
}

fn default_point_limit() -> usize {
    100
}

/// Runtime launch config consumed by the Modbus protocol driver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusServiceLaunchConfig {
    pub transport: ModbusTransportLaunch,
    #[serde(default)]
    pub profile: Option<SimulatorProfile>,
    #[serde(default)]
    pub devices: Option<usize>,
    #[serde(default)]
    pub points_per_device: Option<usize>,
}

impl ModbusServiceLaunchConfig {
    fn from_session(
        defaults: &SimulatorDefaults,
        transport: &TransportDefinition,
        profile: SimulatorProfile,
    ) -> Self {
        let transport = match transport {
            TransportDefinition::Tcp {
                bind,
                port,
                performance_preset,
            } => {
                let bind_address: SocketAddr = format!("{}:{}", bind, port)
                    .parse()
                    .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], *port)));
                ModbusTransportLaunch::Tcp {
                    bind_addr: bind_address,
                    performance_preset: performance_preset
                        .unwrap_or(defaults.tcp_performance_preset),
                }
            }
            TransportDefinition::Rtu { config } => {
                let mut config = config.clone();
                if matches!(config.performance_preset, RtuPerformancePreset::Default) {
                    config.performance_preset = defaults.rtu_performance_preset;
                }
                ModbusTransportLaunch::Rtu { config }
            }
        };

        Self {
            transport,
            profile: Some(profile),
            devices: None,
            points_per_device: None,
        }
    }
}

/// Per-service transport launch configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModbusTransportLaunch {
    Tcp {
        bind_addr: SocketAddr,
        #[serde(default)]
        performance_preset: TcpPerformancePreset,
    },
    Rtu {
        #[serde(default)]
        config: RtuServerConfig,
    },
}

/// Compiled runtime-ready session.
#[derive(Debug, Clone)]
pub struct CompiledModbusSession {
    pub session_name: String,
    pub launch: ProtocolLaunchSpec,
    pub transport_kind: CompiledTransportKind,
    pub profile: SimulatorProfile,
    pub trace: SessionTraceConfig,
    pub reset: SessionResetPolicy,
    pub control: SessionControlConfig,
    pub fault_presets: BTreeMap<String, FaultInjectionConfig>,
    pub active_fault_preset: Option<String>,
    pub response_profiles: BTreeMap<String, ResponseProfileDefinition>,
    pub active_response_profile: Option<String>,
    pub actions: BTreeMap<String, ActionDefinition>,
    pub behaviors: BTreeMap<String, BehaviorDefinition>,
    pub behavior_sets: BTreeMap<String, BehaviorSetDefinition>,
    pub active_behavior_set: Option<String>,
    pub point_catalog: BTreeMap<String, CompiledPointMetadata>,
    pub datastore_policies: Vec<DatastorePolicySummary>,
    pub action_binding_summaries: Vec<ActionBindingSummary>,
    pub behavior_binding_summaries: Vec<BehaviorBindingSummary>,
    pub compiled_behavior_bindings: Vec<CompiledBehaviorBinding>,
    pub readiness_timeout_ms: Option<u64>,
}

impl CompiledModbusSession {
    /// Returns the runtime extensions implied by the compiled session.
    pub fn runtime_extensions(&self) -> RuntimeExtensions {
        let mut extensions = RuntimeExtensions::default();
        if let Some(config) = self.active_runtime_fault_config() {
            extensions.insert_protocol_config(
                "modbus",
                json!({
                    "fault_injection": config,
                }),
            );
        }
        if let Some(layer) = BehaviorLayer::from_compiled(self) {
            extensions.add_device_layer(Arc::new(layer));
        }
        extensions
    }

    /// Returns a cloned session with a different active fault preset.
    pub fn with_active_fault_preset(&self, preset: Option<&str>) -> ModbusResult<Self> {
        if let Some(name) = preset {
            if !self.fault_presets.contains_key(name) {
                return Err(ModbusError::Config(format!(
                    "unknown fault preset '{}'",
                    name
                )));
            }
        }

        let mut cloned = self.clone();
        cloned.active_fault_preset = preset.map(|value| value.to_string());
        Ok(cloned)
    }

    /// Returns a cloned session with a different active response profile.
    pub fn with_active_response_profile(&self, profile: Option<&str>) -> ModbusResult<Self> {
        if let Some(name) = profile {
            if !self.response_profiles.contains_key(name) {
                return Err(ModbusError::Config(format!(
                    "unknown response profile '{}'",
                    name
                )));
            }
        }

        let mut cloned = self.clone();
        cloned.active_response_profile = profile.map(|value| value.to_string());
        Ok(cloned)
    }

    /// Returns a cloned session with a different active behavior set.
    pub fn with_active_behavior_set(&self, behavior_set: Option<&str>) -> ModbusResult<Self> {
        if let Some(name) = behavior_set {
            if !self.behavior_sets.contains_key(name) {
                return Err(ModbusError::Config(format!(
                    "unknown behavior set '{}'",
                    name
                )));
            }
        }

        let mut cloned = self.clone();
        cloned.active_behavior_set = behavior_set.map(|value| value.to_string());
        Ok(cloned)
    }

    /// Returns the currently active fault injection config, if one exists.
    pub fn active_fault_config(&self) -> Option<FaultInjectionConfig> {
        self.active_fault_preset
            .as_ref()
            .and_then(|name| self.fault_presets.get(name).cloned())
    }

    /// Returns the currently active response profile, if one exists.
    pub fn active_response_profile_definition(&self) -> Option<ResponseProfileDefinition> {
        self.active_response_profile
            .as_ref()
            .and_then(|name| self.response_profiles.get(name).cloned())
    }

    /// Returns the merged runtime fault config implied by fault presets and response profiles.
    pub fn active_runtime_fault_config(&self) -> Option<FaultInjectionConfig> {
        let mut merged = FaultInjectionConfig::new();
        let mut enabled = false;
        let mut has_source = false;

        if let Some(config) = self.active_fault_config() {
            has_source = true;
            enabled |= config.enabled;
            merged.faults.extend(config.faults);
        }
        if let Some(config) = self
            .active_response_profile_definition()
            .and_then(|profile| profile.to_fault_injection_config(self.transport_kind))
        {
            has_source = true;
            enabled |= config.enabled;
            merged.faults.extend(config.faults);
        }

        if !has_source {
            None
        } else {
            merged.enabled = enabled;
            Some(merged)
        }
    }

    pub fn point_metadata(&self, device_id: &str, point_id: &str) -> Option<&CompiledPointMetadata> {
        self.point_catalog
            .get(&point_catalog_key(device_id, point_id))
    }
}

/// Inspection summary used by the CLI.
#[derive(Debug, Clone, Serialize)]
pub struct ModbusConfigSummary {
    pub transports: Vec<String>,
    pub datastores: Vec<String>,
    pub devices: Vec<String>,
    pub sessions: Vec<SessionSummary>,
    pub presets: Vec<String>,
    pub actions: Vec<String>,
    pub behaviors: Vec<String>,
    pub response_profiles: Vec<String>,
}

/// Inspection summary for a single named session.
#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub name: String,
    pub transport: String,
    pub devices: Vec<String>,
    pub preset: Option<String>,
    pub active_fault_preset: Option<String>,
    pub active_response_profile: Option<String>,
    pub active_behavior_set: Option<String>,
}

/// Session transport kind compiled alongside immutable runtime metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledTransportKind {
    Tcp,
    Rtu,
}

/// Compiled point metadata surfaced through the control plane.
#[derive(Debug, Clone, Serialize)]
pub struct CompiledPointMetadata {
    pub device_id: String,
    pub point_id: String,
    pub source_datastore: Option<String>,
    pub read_only: bool,
    pub invalid: bool,
    pub action_bindings: Vec<String>,
    pub behavior_bindings: Vec<String>,
}

/// Summary of a compiled datastore policy.
#[derive(Debug, Clone, Serialize)]
pub struct DatastorePolicySummary {
    pub name: String,
    pub kind: String,
    pub invalid_ranges: usize,
    pub readonly_ranges: usize,
    pub typed_blocks: usize,
    pub has_default_value: bool,
    pub repeat: Option<DatastoreRepeatPolicy>,
    pub initialization: Option<String>,
}

/// Summary of action bindings compiled into a session.
#[derive(Debug, Clone, Serialize)]
pub struct ActionBindingSummary {
    pub device_id: String,
    pub point_id: String,
    pub bindings: Vec<String>,
}

/// Summary of behavior bindings compiled into a session.
#[derive(Debug, Clone, Serialize)]
pub struct BehaviorBindingSummary {
    pub device_id: String,
    pub point_id: String,
    pub behavior: String,
    pub behavior_set: String,
    pub trigger: BehaviorTrigger,
}

/// Runtime-ready compiled behavior binding.
#[derive(Debug, Clone)]
pub struct CompiledBehaviorBinding {
    pub name: String,
    pub behavior_set: String,
    pub device_id: String,
    pub point_id: String,
    pub trigger: BehaviorTrigger,
    pub condition: Option<BehaviorCondition>,
    pub interval_ms: Option<u64>,
    pub actions: Vec<String>,
}

struct CompiledSessionMetadata {
    point_catalog: BTreeMap<String, CompiledPointMetadata>,
    datastore_policies: Vec<DatastorePolicySummary>,
    action_bindings: Vec<ActionBindingSummary>,
    behavior_bindings: Vec<BehaviorBindingSummary>,
    compiled_behavior_bindings: Vec<CompiledBehaviorBinding>,
}

#[derive(Debug, Clone)]
struct MatchedBehaviorTarget {
    device_id: String,
    point_id: String,
}

fn point_catalog_key(device_id: &str, point_id: &str) -> String {
    format!("{}/{}", device_id, point_id)
}

fn matching_behavior_targets(
    behavior: &BehaviorDefinition,
    profile: &SimulatorProfile,
) -> Vec<MatchedBehaviorTarget> {
    let mut matches = Vec::new();
    for unit in &profile.units {
        let device_id = format!("modbus-{}", unit.unit_id);
        if let Some(expected_device) = &behavior.target.device_id {
            if &device_id != expected_device {
                continue;
            }
        }
        if let Some(expected_unit) = behavior.target.unit_id {
            if unit.unit_id != expected_unit {
                continue;
            }
        }
        if unit
            .points
            .iter()
            .any(|point| point.id == behavior.target.point_id)
        {
            matches.push(MatchedBehaviorTarget {
                device_id,
                point_id: behavior.target.point_id.clone(),
            });
        }
    }
    matches
}

/// Typed schema surface used by `inspect modbus-schema`.
#[derive(Debug, Clone, Serialize)]
pub struct ModbusSchemaSummary {
    pub kind: &'static str,
    pub formats: Vec<&'static str>,
    pub top_level_sections: Vec<SchemaSection>,
    pub commands: Vec<&'static str>,
    pub notes: Vec<&'static str>,
}

/// Schema description for a top-level section.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaSection {
    pub name: &'static str,
    pub purpose: &'static str,
    pub required: bool,
}

/// Returns the DX-oriented schema summary for Modbus config files.
pub fn schema_summary() -> ModbusSchemaSummary {
    ModbusSchemaSummary {
        kind: "modbus_simulator_config",
        formats: vec!["yaml", "json", "toml"],
        top_level_sections: vec![
            SchemaSection {
                name: "defaults",
                purpose: "Shared trace and readiness defaults used during session compilation",
                required: false,
            },
            SchemaSection {
                name: "transports",
                purpose: "Named TCP or RTU endpoints selected by sessions",
                required: true,
            },
            SchemaSection {
                name: "datastores",
                purpose: "Named dense or sparse datastore definitions reused by devices and presets",
                required: false,
            },
            SchemaSection {
                name: "devices",
                purpose: "Named unit bundles with datastore refs, point catalogs, timing, and tags",
                required: false,
            },
            SchemaSection {
                name: "actions",
                purpose: "Deterministic built-in action catalog referenced by point bindings",
                required: false,
            },
            SchemaSection {
                name: "behaviors",
                purpose: "Deterministic behavior graph referencing actions, triggers, conditions, and point targets",
                required: false,
            },
            SchemaSection {
                name: "sessions",
                purpose: "Canonical run targets selecting transport, devices, control defaults, trace, reset, fault presets, response profiles, and behavior sets",
                required: true,
            },
            SchemaSection {
                name: "response_profiles",
                purpose: "Named wire-level response behaviors compiled into runtime fault policies",
                required: false,
            },
            SchemaSection {
                name: "presets",
                purpose: "Quickstart generators that compile to generated profiles",
                required: false,
            },
        ],
        commands: vec![
            "mabi validate modbus-config <file>",
            "mabi inspect modbus-config <file>",
            "mabi serve modbus --config <file> --session <name>",
            "mabi control modbus ...",
        ],
        notes: vec![
            "Config files are source-of-truth and stay file-backed",
            "Runtime mutations do not rewrite config files",
            "Named fault presets are scoped to a session definition",
            "Deterministic actions, behaviors, and response profiles compile into immutable session metadata",
        ],
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        default_trace_capacity, CompiledModbusSession, CompiledTransportKind,
        DatastoreDefinition, DatastoreSelector, GeneratedPresetDefinition, ModbusSimulatorConfig,
        ResponseProfileDefinition, SessionControlConfig, SessionDefinition, SessionTraceConfig,
        SimulatorDefaults, TransportDefinition,
    };
    use crate::fault_injection::FaultInjectionConfig;
    use crate::profile::SimulatorProfile;

    #[test]
    fn config_parses_yaml_and_compiles_session() {
        let config = ModbusSimulatorConfig::from_str_with_format(
            r#"
defaults:
  trace_capacity: 128
transports:
  local:
    kind: tcp
    bind: 127.0.0.1
    port: 1502
devices:
  plant:
    units:
      - unit_id: 1
        name: Pump
sessions:
  demo:
    transport: local
    devices: [plant]
"#,
            "yaml",
        )
        .unwrap();

        let compiled = config.compile_session("demo").unwrap();
        assert_eq!(compiled.session_name, "demo");
        assert!(compiled.launch.config["profile"]["units"].is_array());
        assert_eq!(compiled.trace.buffer_capacity(), 128);
    }

    #[test]
    fn config_rejects_unknown_transport_reference() {
        let config = ModbusSimulatorConfig {
            sessions: BTreeMap::from([(
                "broken".to_string(),
                SessionDefinition {
                    transport: "missing".to_string(),
                    service_name: None,
                    devices: vec![],
                    preset: Some("default".to_string()),
                    trace: SessionTraceConfig::default(),
                    reset: Default::default(),
                    control: Default::default(),
                    fault_presets: BTreeMap::new(),
                    active_fault_preset: None,
                    active_response_profile: None,
                    behavior_sets: BTreeMap::new(),
                    active_behavior_set: None,
                    readiness_timeout_ms: None,
                },
            )]),
            presets: BTreeMap::from([(
                "default".to_string(),
                GeneratedPresetDefinition {
                    devices: 1,
                    points_per_device: 4,
                    datastore: None,
                },
            )]),
            ..Default::default()
        };

        let error = config.validate().unwrap_err().to_string();
        assert!(error.contains("unknown transport"));
    }

    #[test]
    fn compiled_session_runtime_extensions_include_active_fault_preset() {
        let compiled = CompiledModbusSession {
            session_name: "demo".into(),
            launch: mabi_runtime::ProtocolLaunchSpec {
                protocol: "modbus".into(),
                name: Some("demo".into()),
                config: serde_json::json!({}),
            },
            transport_kind: CompiledTransportKind::Tcp,
            profile: SimulatorProfile::new(),
            trace: SessionTraceConfig {
                enabled: true,
                capacity: Some(default_trace_capacity()),
            },
            reset: Default::default(),
            control: SessionControlConfig::default(),
            fault_presets: BTreeMap::from([("delay".into(), FaultInjectionConfig::default())]),
            active_fault_preset: Some("delay".into()),
            response_profiles: BTreeMap::new(),
            active_response_profile: None,
            actions: BTreeMap::new(),
            behaviors: BTreeMap::new(),
            behavior_sets: BTreeMap::new(),
            active_behavior_set: None,
            point_catalog: BTreeMap::new(),
            datastore_policies: Vec::new(),
            action_binding_summaries: Vec::new(),
            behavior_binding_summaries: Vec::new(),
            compiled_behavior_bindings: Vec::new(),
            readiness_timeout_ms: None,
        };

        let extensions = compiled.runtime_extensions();
        assert!(extensions.protocol_config("modbus").is_some());
    }

    #[test]
    fn generated_preset_can_override_datastore_kind() {
        let preset = GeneratedPresetDefinition {
            devices: 1,
            points_per_device: 4,
            datastore: Some(DatastoreSelector::Inline(DatastoreDefinition::from(
                crate::profile::DatastoreKind::Sparse {
                    config: Default::default(),
                },
            ))),
        };

        let profile = preset.build(&BTreeMap::new()).unwrap();
        assert!(matches!(
            profile.units[0].datastore,
            crate::profile::DatastoreKind::Sparse { .. }
        ));
    }

    #[test]
    fn compiled_session_runtime_extensions_include_active_response_profile_faults() {
        let compiled = CompiledModbusSession {
            session_name: "demo".into(),
            launch: mabi_runtime::ProtocolLaunchSpec {
                protocol: "modbus".into(),
                name: Some("demo".into()),
                config: serde_json::json!({}),
            },
            transport_kind: CompiledTransportKind::Tcp,
            profile: SimulatorProfile::new(),
            trace: SessionTraceConfig::default(),
            reset: Default::default(),
            control: SessionControlConfig::default(),
            fault_presets: BTreeMap::new(),
            active_fault_preset: None,
            response_profiles: BTreeMap::from([(
                "slow".into(),
                ResponseProfileDefinition {
                    delay_ms: Some(25),
                    ..Default::default()
                },
            )]),
            active_response_profile: Some("slow".into()),
            actions: BTreeMap::new(),
            behaviors: BTreeMap::new(),
            behavior_sets: BTreeMap::new(),
            active_behavior_set: None,
            point_catalog: BTreeMap::new(),
            datastore_policies: Vec::new(),
            action_binding_summaries: Vec::new(),
            behavior_binding_summaries: Vec::new(),
            compiled_behavior_bindings: Vec::new(),
            readiness_timeout_ms: None,
        };

        let extensions = compiled.runtime_extensions();
        let config = extensions.protocol_config("modbus").unwrap();
        assert_eq!(config["fault_injection"]["faults"][0]["type"], "delayed_response");
    }

    #[test]
    fn named_datastore_references_compile_into_profiles() {
        let config = ModbusSimulatorConfig::from_str_with_format(
            r#"
transports:
  local:
    kind: tcp
    bind: 127.0.0.1
    port: 1502
datastores:
  sparse_lab:
    kind: sparse
devices:
  lab:
    units:
      - unit_id: 7
        name: TestBench
        datastore: sparse_lab
sessions:
  demo:
    transport: local
    devices: [lab]
"#,
            "yaml",
        )
        .unwrap();

        let compiled = config.compile_session("demo").unwrap();
        assert!(matches!(
            compiled.profile.units[0].datastore,
            crate::profile::DatastoreKind::Sparse { .. }
        ));
    }

    #[test]
    fn config_compiles_action_bindings_and_point_catalog_metadata() {
        let config = ModbusSimulatorConfig::from_str_with_format(
            r#"
transports:
  local:
    kind: tcp
    bind: 127.0.0.1
    port: 1502
datastores:
  sparse_named:
    kind: sparse
    readonly_ranges:
      - register_type: holding_register
        start: 5
        quantity: 1
    invalid_ranges:
      - register_type: holding_register
        start: 8
        quantity: 1
actions:
  clamp_temp:
    kind: clamp
    min: 0
    max: 100
response_profiles:
  slow:
    delay_ms: 10
devices:
  plant:
    units:
      - unit_id: 1
        name: Pump
        datastore: sparse_named
        points:
          - id: temperature
            name: Temperature
            register_type: holding_register
            address: 5
            data_type: uint16
        action_bindings:
          - point_id: temperature
            bindings:
              - action: clamp_temp
                trigger: on_write
sessions:
  demo:
    transport: local
    devices: [plant]
    active_response_profile: slow
"#,
            "yaml",
        )
        .unwrap();

        let compiled = config.compile_session("demo").unwrap();
        let metadata = compiled
            .point_metadata("modbus-1", "temperature")
            .expect("compiled point metadata");
        assert_eq!(metadata.source_datastore.as_deref(), Some("sparse_named"));
        assert!(metadata.read_only);
        assert_eq!(metadata.action_bindings, vec!["clamp_temp@on_write"]);
        assert_eq!(compiled.active_response_profile.as_deref(), Some("slow"));
    }

    #[test]
    fn schema_defaults_are_stable() {
        let defaults = SimulatorDefaults::default();
        assert_eq!(defaults.trace_capacity, default_trace_capacity());
        assert!(matches!(
            TransportDefinition::Tcp {
                bind: "0.0.0.0".into(),
                port: 502,
                performance_preset: None,
            },
            TransportDefinition::Tcp { .. }
        ));
    }
}
