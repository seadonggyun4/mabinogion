//! Canonical session-centric modeling surface for the OPC UA simulator.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use mabi_core::tags::Tags;
use mabi_core::types::{AccessMode, Address, DataPointDef, DataType};
use mabi_core::value::Value;
use mabi_runtime::{ProtocolLaunchSpec, RuntimeExtensions};

use crate::config::OpcUaServerConfig;
use crate::error::{OpcUaError, OpcUaResult};
use crate::nodes::base::{LocalizedText, QualifiedName};
use crate::nodes::classes::{
    DataTypeNode, MethodNode, ObjectNode, ObjectTypeNode, ReferenceTypeNode, VariableNode,
    VariableTypeNode, ViewNode,
};
use crate::nodes::{AddressSpace, Reference, ReferenceDirection, ReferenceTypeId};
use crate::types::{AccessLevel, NodeId, NodeIdType, Variant};

const STANDARD_NAMESPACE_URI: &str = "http://opcfoundation.org/UA/";
const DEFAULT_NAMESPACE_URI: &str = "urn:mabinogion:opcua:simulator";
const DEFAULT_SERVER_NAME: &str = "Mabinogion OPC UA Simulator";
const DEFAULT_ENDPOINT_PATH: &str = "/";

/// Canonical file-backed config surface for the OPC UA simulator.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpcUaSimulatorConfig {
    #[serde(default)]
    pub defaults: SimulatorDefaults,
    #[serde(default)]
    pub transports: BTreeMap<String, TransportDefinition>,
    #[serde(default)]
    pub nodesets: BTreeMap<String, NodeSetSource>,
    #[serde(default)]
    pub models: BTreeMap<String, ModelDefinition>,
    #[serde(default)]
    pub devices: BTreeMap<String, DeviceDefinition>,
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionDefinition>,
    #[serde(default)]
    pub presets: BTreeMap<String, PresetDefinition>,
}

impl OpcUaSimulatorConfig {
    /// Loads a simulator config from YAML, JSON, or TOML.
    pub fn from_path(path: &Path) -> OpcUaResult<Self> {
        let content = fs::read_to_string(path)?;
        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
        Self::from_str_with_format(&content, extension, Some(path))
    }

    /// Parses a simulator config from the supplied content and format hint.
    pub fn from_str_with_format(
        content: &str,
        format: &str,
        base_path: Option<&Path>,
    ) -> OpcUaResult<Self> {
        let parsed: Self = match format {
            "yaml" | "yml" => serde_yaml::from_str(content)
                .map_err(|error| OpcUaError::Config(format!("invalid YAML config: {}", error)))?,
            "json" => serde_json::from_str(content)
                .map_err(|error| OpcUaError::Config(format!("invalid JSON config: {}", error)))?,
            "toml" => toml::from_str(content)
                .map_err(|error| OpcUaError::Config(format!("invalid TOML config: {}", error)))?,
            other => {
                return Err(OpcUaError::Config(format!(
                    "unsupported config format: {}",
                    other
                )))
            }
        };
        parsed.validate(base_path)?;
        Ok(parsed)
    }

    /// Validates all named references and canonical compile-time invariants.
    pub fn validate(&self, base_path: Option<&Path>) -> OpcUaResult<()> {
        if self.sessions.is_empty() {
            return Err(OpcUaError::Config(
                "simulator config must define at least one session".into(),
            ));
        }

        for (name, session) in &self.sessions {
            if !self.transports.contains_key(&session.transport) {
                return Err(OpcUaError::Config(format!(
                    "session '{}' references unknown transport '{}'",
                    name, session.transport
                )));
            }
            if session.models.is_empty() && session.devices.is_empty() && session.preset.is_none() {
                return Err(OpcUaError::Config(format!(
                    "session '{}' must reference at least one model, device, or preset",
                    name
                )));
            }
            for model in &session.models {
                if !self.models.contains_key(model) {
                    return Err(OpcUaError::Config(format!(
                        "session '{}' references unknown model '{}'",
                        name, model
                    )));
                }
            }
            for device in &session.devices {
                if !self.devices.contains_key(device) {
                    return Err(OpcUaError::Config(format!(
                        "session '{}' references unknown device '{}'",
                        name, device
                    )));
                }
            }
            if let Some(preset) = &session.preset {
                if !self.presets.contains_key(preset) {
                    return Err(OpcUaError::Config(format!(
                        "session '{}' references unknown preset '{}'",
                        name, preset
                    )));
                }
            }
        }

        for (name, model) in &self.models {
            for nodeset in &model.nodesets {
                let source = self.nodesets.get(nodeset).ok_or_else(|| {
                    OpcUaError::Config(format!(
                        "model '{}' references unknown nodeset '{}'",
                        name, nodeset
                    ))
                })?;
                validate_nodeset_source(nodeset, source, base_path)?;
            }
            let mut overlay_ids = BTreeSet::new();
            for overlay in &model.overlays {
                let overlay_node_id = match overlay {
                    OverlayNodeDefinition::Folder { node_id, .. }
                    | OverlayNodeDefinition::Object { node_id, .. }
                    | OverlayNodeDefinition::Variable { node_id, .. } => node_id,
                };
                if !overlay_ids.insert(overlay_node_id.clone()) {
                    return Err(OpcUaError::Config(format!(
                        "model '{}' contains duplicate overlay node '{}'",
                        name, overlay_node_id
                    )));
                }
            }
        }

        for (name, device) in &self.devices {
            if !self.models.contains_key(&device.model) {
                return Err(OpcUaError::Config(format!(
                    "device '{}' references unknown model '{}'",
                    name, device.model
                )));
            }
        }

        for name in self.sessions.keys() {
            self.compile_session(name, base_path)?;
        }

        Ok(())
    }

    /// Compiles a named session into a stable launch spec and generated catalog.
    pub fn compile_session(
        &self,
        name: &str,
        base_path: Option<&Path>,
    ) -> OpcUaResult<CompiledOpcUaSession> {
        let session = self
            .sessions
            .get(name)
            .ok_or_else(|| OpcUaError::Config(format!("unknown session '{}'", name)))?;
        let transport = self.transports.get(&session.transport).ok_or_else(|| {
            OpcUaError::Config(format!(
                "session '{}' references unknown transport '{}'",
                name, session.transport
            ))
        })?;

        let mut synthetic_models = BTreeMap::new();
        let mut synthetic_devices = BTreeMap::new();
        if let Some(preset_name) = &session.preset {
            let preset = self
                .presets
                .get(preset_name)
                .ok_or_else(|| OpcUaError::Config(format!("unknown preset '{}'", preset_name)))?;
            let (model_name, model, device_name, device) = preset.compile(
                preset_name,
                &self.defaults,
                session.service_name.as_deref().unwrap_or(name),
            );
            synthetic_models.insert(model_name.clone(), model);
            synthetic_devices.insert(device_name.clone(), device);
        }

        let mut ordered_model_names = Vec::new();
        let mut seen_models = BTreeSet::new();
        for model_name in &session.models {
            if seen_models.insert(model_name.clone()) {
                ordered_model_names.push(model_name.clone());
            }
        }
        for device_name in &session.devices {
            let device = self
                .devices
                .get(device_name)
                .ok_or_else(|| OpcUaError::Config(format!("unknown device '{}'", device_name)))?;
            if seen_models.insert(device.model.clone()) {
                ordered_model_names.push(device.model.clone());
            }
        }
        for device in synthetic_devices.values() {
            if seen_models.insert(device.model.clone()) {
                ordered_model_names.push(device.model.clone());
            }
        }

        let mut namespace_table = vec![
            STANDARD_NAMESPACE_URI.to_string(),
            self.defaults.namespace_uri.clone(),
        ];
        let mut plan_sources = Vec::new();
        let mut imported_nodesets = Vec::new();

        for model_name in &ordered_model_names {
            let model = self
                .models
                .get(model_name)
                .or_else(|| synthetic_models.get(model_name))
                .ok_or_else(|| OpcUaError::Config(format!("unknown model '{}'", model_name)))?;
            if let Some(namespace_uri) = &model.namespace_uri {
                push_unique(&mut namespace_table, namespace_uri.clone());
            }
            for companion in &model.companions {
                if let Some(namespace_uri) = &companion.namespace_uri {
                    push_unique(&mut namespace_table, namespace_uri.clone());
                }
            }
            for nodeset_name in &model.nodesets {
                let source = self.nodesets.get(nodeset_name).ok_or_else(|| {
                    OpcUaError::Config(format!(
                        "model '{}' references unknown nodeset '{}'",
                        model_name, nodeset_name
                    ))
                })?;
                let imported = import_nodeset_source(source, base_path)?;
                for uri in imported.local_namespace_table.iter().skip(1) {
                    push_unique(&mut namespace_table, uri.clone());
                }
                imported_nodesets.push((model_name.clone(), nodeset_name.clone(), imported));
                plan_sources.push(format!("model:{} -> nodeset:{}", model_name, nodeset_name));
            }
        }

        let mut node_map = BTreeMap::<String, GeneratedNodeDefinition>::new();
        let mut references = Vec::new();
        let mut methods = Vec::new();
        let mut events = Vec::new();

        for (model_name, nodeset_name, imported) in &imported_nodesets {
            let mapping = namespace_mapping(&imported.local_namespace_table, &namespace_table)?;
            for node in &imported.nodes {
                let remapped = node.remap_namespaces(&mapping)?;
                let key = remapped.node_id().to_string();
                if node_map.insert(key.clone(), remapped).is_some() {
                    return Err(OpcUaError::Config(format!(
                        "imported nodes collide on final NodeId '{}'",
                        key
                    )));
                }
            }
            for reference in &imported.references {
                references.push(reference.remap_namespaces(&mapping)?);
            }
            plan_sources.push(format!("import:{}:{}", model_name, nodeset_name));
        }

        for model_name in &ordered_model_names {
            let model = self
                .models
                .get(model_name)
                .or_else(|| synthetic_models.get(model_name))
                .ok_or_else(|| OpcUaError::Config(format!("unknown model '{}'", model_name)))?;

            for overlay in &model.overlays {
                let node = overlay.compile()?;
                let key = node.node_id().to_string();
                if node_map.insert(key.clone(), node).is_some() {
                    return Err(OpcUaError::Config(format!(
                        "model '{}' overlay collides on final NodeId '{}'",
                        model_name, key
                    )));
                }
            }
            for reference in &model.references {
                references.push(reference.compile()?);
            }
            for method in &model.methods {
                let (node, structural_reference) = method.compile()?;
                let key = node.node_id().to_string();
                if node_map.insert(key.clone(), node).is_some() {
                    return Err(OpcUaError::Config(format!(
                        "model '{}' method collides on final NodeId '{}'",
                        model_name, key
                    )));
                }
                if let Some(reference) = structural_reference {
                    references.push(reference);
                }
                methods.push(method.clone());
            }
            events.extend(model.events.clone());
        }

        for reference in &references {
            if !node_map.contains_key(&reference.source_node_id.to_string())
                && !is_standard_node(&reference.source_node_id)
            {
                return Err(OpcUaError::Config(format!(
                    "reference source '{}' does not exist",
                    reference.source_node_id
                )));
            }
            if !node_map.contains_key(&reference.target_node_id.to_string())
                && !is_standard_node(&reference.target_node_id)
            {
                return Err(OpcUaError::Config(format!(
                    "reference target '{}' does not exist",
                    reference.target_node_id
                )));
            }
        }

        let mut compiled_devices = Vec::new();
        if session.devices.is_empty() && synthetic_devices.is_empty() {
            for model_name in &ordered_model_names {
                let model = self
                    .models
                    .get(model_name)
                    .or_else(|| synthetic_models.get(model_name))
                    .ok_or_else(|| OpcUaError::Config(format!("unknown model '{}'", model_name)))?;
                let auto_device = DeviceDefinition {
                    model: model_name.clone(),
                    node_bindings: Vec::new(),
                    tags: Tags::new().with_tag("model", model_name.clone()),
                    name: Some(model.display_name(model_name)),
                };
                compiled_devices.push(compile_device(
                    &format!("device-{}", model_name),
                    &auto_device,
                    &node_map,
                )?);
            }
        } else {
            for device_name in &session.devices {
                let device = self.devices.get(device_name).ok_or_else(|| {
                    OpcUaError::Config(format!("unknown device '{}'", device_name))
                })?;
                compiled_devices.push(compile_device(device_name, device, &node_map)?);
            }
            for (device_name, device) in &synthetic_devices {
                compiled_devices.push(compile_device(device_name, device, &node_map)?);
            }
        }

        let mut point_ids = BTreeSet::new();
        for device in &compiled_devices {
            for point in &device.points {
                if !point_ids.insert(point.point_id.clone()) {
                    return Err(OpcUaError::Config(format!(
                        "compiled session '{}' contains duplicate point id '{}'",
                        name, point.point_id
                    )));
                }
            }
        }

        let catalog = GeneratedNodeCatalog {
            namespace_table: namespace_table.clone(),
            namespace_plan: NamespaceCompilationPlan {
                namespaces: namespace_table.clone(),
                sources: plan_sources,
                models: ordered_model_names.clone(),
            },
            nodes: node_map.into_values().collect(),
            references,
            type_tree_seeds: collect_type_tree_seeds(&namespace_table),
            methods,
            events,
            point_bindings: compiled_devices
                .iter()
                .flat_map(|device| device.points.iter().cloned())
                .collect(),
        };

        let compiled_launch = OpcUaCompiledLaunchConfig {
            session_name: name.to_string(),
            server_config: build_server_config(&self.defaults, session, transport),
            catalog: catalog.clone(),
            devices: compiled_devices.clone(),
            control: session.control.clone(),
            readiness_timeout_ms: session
                .readiness_timeout_ms
                .or(self.defaults.readiness_timeout_ms),
        };

        let launch = ProtocolLaunchSpec {
            protocol: "opcua".into(),
            name: Some(
                session
                    .service_name
                    .clone()
                    .unwrap_or_else(|| name.to_string()),
            ),
            config: serde_json::to_value(&compiled_launch)
                .map_err(|error| OpcUaError::Config(error.to_string()))?,
        };

        Ok(CompiledOpcUaSession {
            session_name: name.to_string(),
            launch,
            namespace_plan: catalog.namespace_plan.clone(),
            catalog,
            devices: compiled_devices,
            control: session.control.clone(),
            readiness_timeout_ms: compiled_launch.readiness_timeout_ms,
        })
    }

    /// Returns a stable inspection summary for CLI surfaces.
    pub fn inspect_summary(&self) -> OpcUaConfigSummary {
        OpcUaConfigSummary {
            transports: self.transports.keys().cloned().collect(),
            nodesets: self.nodesets.keys().cloned().collect(),
            models: self.models.keys().cloned().collect(),
            devices: self.devices.keys().cloned().collect(),
            sessions: self
                .sessions
                .iter()
                .map(|(name, session)| OpcUaSessionSummary {
                    name: name.clone(),
                    transport: session.transport.clone(),
                    models: session.models.clone(),
                    devices: session.devices.clone(),
                    preset: session.preset.clone(),
                    service_name: session.service_name.clone(),
                })
                .collect(),
            presets: self.presets.keys().cloned().collect(),
        }
    }
}

/// Loads a simulator config from the supplied path.
pub fn load_simulator_config(path: &Path) -> OpcUaResult<OpcUaSimulatorConfig> {
    OpcUaSimulatorConfig::from_path(path)
}

/// Compiles a named session from the supplied config.
pub fn compile_session(
    config: &OpcUaSimulatorConfig,
    session_name: &str,
    base_path: Option<&Path>,
) -> OpcUaResult<CompiledOpcUaSession> {
    config.compile_session(session_name, base_path)
}

/// Returns the canonical schema summary for CLI inspection.
pub fn schema_summary() -> OpcUaSchemaSummary {
    OpcUaSchemaSummary {
        kind: "opcua_simulator",
        formats: vec!["yaml", "json", "toml"],
        top_level_sections: vec![
            SchemaSection::new(
                "defaults",
                false,
                "Runtime-wide default namespace and server settings",
            ),
            SchemaSection::new("transports", true, "Named OPC UA endpoint definitions"),
            SchemaSection::new("nodesets", false, "NodeSet2 import sources"),
            SchemaSection::new("models", false, "Address-space composition and overlays"),
            SchemaSection::new("devices", false, "Runtime-visible point bindings"),
            SchemaSection::new("sessions", true, "Named runtime sessions"),
            SchemaSection::new("presets", false, "Legacy convenience generation"),
        ],
        commands: vec![
            "mabi inspect opcua-schema",
            "mabi inspect opcua-config <file>",
            "mabi validate opcua-config <file>",
            "mabi serve opcua --config <file> --session <name>",
            "mabi control opcua --config <file> --session <name> ...",
        ],
        notes: vec![
            "NodeSet2 imports are runtime-loaded and deterministic",
            "Remote fetch and scripting are intentionally unsupported",
            "Legacy builder and numeric serve paths compile into ephemeral sessions",
        ],
    }
}

/// Stable config inspection summary.
pub fn inspect_summary(config: &OpcUaSimulatorConfig) -> OpcUaConfigSummary {
    config.inspect_summary()
}

/// Default simulator-wide settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatorDefaults {
    #[serde(default = "default_namespace_uri")]
    pub namespace_uri: String,
    #[serde(default)]
    pub readiness_timeout_ms: Option<u64>,
    #[serde(default = "default_server_name")]
    pub server_name: String,
    #[serde(default)]
    pub min_publishing_interval_ms: Option<u32>,
    #[serde(default)]
    pub security_profile: Option<String>,
}

impl Default for SimulatorDefaults {
    fn default() -> Self {
        Self {
            namespace_uri: default_namespace_uri(),
            readiness_timeout_ms: Some(5_000),
            server_name: default_server_name(),
            min_publishing_interval_ms: Some(100),
            security_profile: Some("None".into()),
        }
    }
}

/// Named transport definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportDefinition {
    #[serde(default = "default_bind_address")]
    pub bind: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_endpoint_path")]
    pub endpoint_path: String,
    #[serde(default)]
    pub security_profile: Option<String>,
    #[serde(default)]
    pub server_name: Option<String>,
}

impl Default for TransportDefinition {
    fn default() -> Self {
        Self {
            bind: default_bind_address(),
            port: default_port(),
            endpoint_path: default_endpoint_path(),
            security_profile: Some("None".into()),
            server_name: None,
        }
    }
}

/// File-backed NodeSet2 source or embedded alias.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeSetSource {
    File {
        path: PathBuf,
        #[serde(default)]
        namespace_uri_override: Option<String>,
    },
    Embedded {
        alias: String,
        #[serde(default)]
        namespace_uri_override: Option<String>,
    },
}

/// Optional companion-model metadata reference.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompanionModelRef {
    pub name: String,
    #[serde(default)]
    pub namespace_uri: Option<String>,
}

/// Address-space composition unit.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelDefinition {
    #[serde(default)]
    pub nodesets: Vec<String>,
    #[serde(default)]
    pub namespace_uri: Option<String>,
    #[serde(default)]
    pub companions: Vec<CompanionModelRef>,
    #[serde(default)]
    pub overlays: Vec<OverlayNodeDefinition>,
    #[serde(default)]
    pub references: Vec<ReferenceDefinition>,
    #[serde(default)]
    pub methods: Vec<MethodDefinition>,
    #[serde(default)]
    pub events: Vec<EventDefinition>,
}

impl ModelDefinition {
    fn display_name(&self, fallback: &str) -> String {
        fallback.replace('-', " ")
    }
}

/// Runtime-visible device bundle over a model.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceDefinition {
    pub model: String,
    #[serde(default)]
    pub node_bindings: Vec<NodeBindingDefinition>,
    #[serde(default, skip_serializing_if = "Tags::is_empty")]
    pub tags: Tags,
    #[serde(default)]
    pub name: Option<String>,
}

/// Stable point binding onto a concrete OPC UA node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeBindingDefinition {
    pub point_id: String,
    pub node_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub writable: Option<bool>,
    #[serde(default)]
    pub historizing: Option<bool>,
    #[serde(default)]
    pub sampling_interval_ms: Option<u32>,
    #[serde(default)]
    pub seed: Option<Value>,
    #[serde(default, skip_serializing_if = "Tags::is_empty")]
    pub tags: Tags,
}

/// Session-scoped control defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionControlConfig {
    #[serde(default = "default_true")]
    pub allow_raw_node_access: bool,
}

impl Default for SessionControlConfig {
    fn default() -> Self {
        Self {
            allow_raw_node_access: true,
        }
    }
}

/// Named execution unit.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionDefinition {
    pub transport: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub readiness_timeout_ms: Option<u64>,
    #[serde(default)]
    pub control: SessionControlConfig,
}

/// Legacy convenience generation preset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetDefinition {
    #[serde(default = "default_preset_nodes")]
    pub nodes: usize,
    #[serde(default = "default_preset_base_node")]
    pub base_node_id: u32,
    #[serde(default = "default_true")]
    pub writable: bool,
    #[serde(default)]
    pub historizing: bool,
    #[serde(default)]
    pub folder_name: Option<String>,
}

impl Default for PresetDefinition {
    fn default() -> Self {
        Self {
            nodes: default_preset_nodes(),
            base_node_id: default_preset_base_node(),
            writable: true,
            historizing: false,
            folder_name: None,
        }
    }
}

impl PresetDefinition {
    fn compile(
        &self,
        preset_name: &str,
        defaults: &SimulatorDefaults,
        service_name: &str,
    ) -> (String, ModelDefinition, String, DeviceDefinition) {
        let folder_id = format!("ns=1;s={}.folder", preset_name);
        let folder_name = self
            .folder_name
            .clone()
            .unwrap_or_else(|| format!("{} Model", service_name));
        let mut overlays = vec![OverlayNodeDefinition::Folder {
            node_id: folder_id.clone(),
            browse_name: folder_name.clone(),
            display_name: Some(folder_name.clone()),
            description: Some("Generated from legacy numeric OPC UA preset".into()),
        }];
        let mut bindings = Vec::new();
        for index in 0..self.nodes {
            let node_id = format!("ns=1;i={}", self.base_node_id + index as u32);
            let browse_name = format!("Variable_{}", index);
            overlays.push(OverlayNodeDefinition::Variable {
                node_id: node_id.clone(),
                browse_name: browse_name.clone(),
                display_name: Some(browse_name.clone()),
                description: Some("Generated variable".into()),
                data_type: Some("i=11".into()),
                value: Some(Value::F64(index as f64 * 0.1)),
                writable: self.writable,
                historizing: self.historizing,
                sampling_interval_ms: defaults.min_publishing_interval_ms,
            });
            bindings.push(NodeBindingDefinition {
                point_id: node_id.clone(),
                node_id,
                label: Some(browse_name),
                writable: Some(self.writable),
                historizing: Some(self.historizing),
                sampling_interval_ms: defaults.min_publishing_interval_ms,
                seed: None,
                tags: Tags::new(),
            });
        }
        let model_name = format!("preset-{}", preset_name);
        let device_name = format!("preset-device-{}", preset_name);
        (
            model_name.clone(),
            ModelDefinition {
                nodesets: Vec::new(),
                namespace_uri: Some(defaults.namespace_uri.clone()),
                companions: Vec::new(),
                overlays,
                references: vec![ReferenceDefinition {
                    source_node_id: NodeId::objects_folder().to_string(),
                    reference_type: ReferenceTypeId::Organizes,
                    target_node_id: folder_id.clone(),
                    direction: ReferenceDirection::Forward,
                }],
                methods: Vec::new(),
                events: Vec::new(),
            },
            device_name,
            DeviceDefinition {
                model: model_name,
                node_bindings: bindings,
                tags: Tags::new()
                    .with_label("generated")
                    .with_tag("preset", preset_name),
                name: Some(format!("{} Generated Device", service_name)),
            },
        )
    }
}

/// Overlay node definition accepted by the canonical config surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverlayNodeDefinition {
    Folder {
        node_id: String,
        browse_name: String,
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        description: Option<String>,
    },
    Object {
        node_id: String,
        browse_name: String,
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        event_notifier: Option<u8>,
    },
    Variable {
        node_id: String,
        browse_name: String,
        #[serde(default)]
        display_name: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        data_type: Option<String>,
        #[serde(default)]
        value: Option<Value>,
        #[serde(default)]
        writable: bool,
        #[serde(default)]
        historizing: bool,
        #[serde(default)]
        sampling_interval_ms: Option<u32>,
    },
}

impl OverlayNodeDefinition {
    fn compile(&self) -> OpcUaResult<GeneratedNodeDefinition> {
        match self {
            Self::Folder {
                node_id,
                browse_name,
                display_name,
                description,
            } => Ok(GeneratedNodeDefinition::Object {
                node_id: parse_node_id(node_id)?,
                browse_name: parse_browse_name(browse_name),
                display_name: localized_text(display_name.as_deref().unwrap_or(browse_name)),
                description: description.clone().map(localized_text_owned),
                event_notifier: 0,
                folder_like: true,
            }),
            Self::Object {
                node_id,
                browse_name,
                display_name,
                description,
                event_notifier,
            } => Ok(GeneratedNodeDefinition::Object {
                node_id: parse_node_id(node_id)?,
                browse_name: parse_browse_name(browse_name),
                display_name: localized_text(display_name.as_deref().unwrap_or(browse_name)),
                description: description.clone().map(localized_text_owned),
                event_notifier: event_notifier.unwrap_or(0),
                folder_like: false,
            }),
            Self::Variable {
                node_id,
                browse_name,
                display_name,
                description,
                data_type,
                value,
                writable,
                historizing,
                sampling_interval_ms,
            } => Ok(GeneratedNodeDefinition::Variable {
                node_id: parse_node_id(node_id)?,
                browse_name: parse_browse_name(browse_name),
                display_name: localized_text(display_name.as_deref().unwrap_or(browse_name)),
                description: description.clone().map(localized_text_owned),
                data_type: data_type
                    .as_ref()
                    .map(|node_id| parse_node_id(node_id))
                    .transpose()?
                    .unwrap_or_else(|| NodeId::numeric(0, 11)),
                value: value.clone().unwrap_or(Value::Null),
                writable: *writable,
                historizing: *historizing,
                sampling_interval_ms: *sampling_interval_ms,
            }),
        }
    }
}

/// Structural reference addition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceDefinition {
    pub source_node_id: String,
    pub reference_type: ReferenceTypeId,
    pub target_node_id: String,
    #[serde(default = "default_reference_direction")]
    pub direction: ReferenceDirection,
}

impl ReferenceDefinition {
    fn compile(&self) -> OpcUaResult<CompiledNodeReference> {
        Ok(CompiledNodeReference {
            source_node_id: parse_node_id(&self.source_node_id)?,
            reference_type: self.reference_type,
            target_node_id: parse_node_id(&self.target_node_id)?,
            direction: self.direction,
        })
    }
}

/// Structural method declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodDefinition {
    pub node_id: String,
    pub parent_id: String,
    pub browse_name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default = "default_true")]
    pub executable: bool,
}

impl MethodDefinition {
    fn compile(&self) -> OpcUaResult<(GeneratedNodeDefinition, Option<CompiledNodeReference>)> {
        Ok((
            GeneratedNodeDefinition::Method {
                node_id: parse_node_id(&self.node_id)?,
                browse_name: parse_browse_name(&self.browse_name),
                display_name: localized_text(
                    self.display_name
                        .as_deref()
                        .unwrap_or(self.browse_name.as_str()),
                ),
                description: None,
                executable: self.executable,
            },
            Some(CompiledNodeReference {
                source_node_id: parse_node_id(&self.parent_id)?,
                reference_type: ReferenceTypeId::HasComponent,
                target_node_id: parse_node_id(&self.node_id)?,
                direction: ReferenceDirection::Forward,
            }),
        ))
    }
}

/// Structural event declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDefinition {
    pub event_type: String,
    #[serde(default)]
    pub source_node_id: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Compiled namespace plan used by runtime materialization.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NamespaceCompilationPlan {
    pub namespaces: Vec<String>,
    pub sources: Vec<String>,
    pub models: Vec<String>,
}

/// Compiled address-space catalog that the runtime can materialize directly.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeneratedNodeCatalog {
    pub namespace_table: Vec<String>,
    pub namespace_plan: NamespaceCompilationPlan,
    pub nodes: Vec<GeneratedNodeDefinition>,
    pub references: Vec<CompiledNodeReference>,
    pub type_tree_seeds: Vec<TypeTreeSeed>,
    pub methods: Vec<MethodDefinition>,
    pub events: Vec<EventDefinition>,
    pub point_bindings: Vec<CompiledPointBinding>,
}

impl GeneratedNodeCatalog {
    /// Materializes the catalog into the provided address space.
    pub fn materialize(&self, address_space: &AddressSpace) -> OpcUaResult<()> {
        for namespace_uri in self.namespace_table.iter().skip(1) {
            address_space.register_namespace(namespace_uri);
        }
        for node in &self.nodes {
            node.insert_into(address_space);
        }
        for reference in &self.references {
            address_space.add_reference(reference.as_reference());
        }
        Ok(())
    }

    /// Returns a stable namespace summary.
    pub fn namespace_summary(&self) -> Vec<String> {
        self.namespace_table.clone()
    }
}

/// Typed compiled session shared by runtime and control flows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledOpcUaSession {
    pub session_name: String,
    pub launch: ProtocolLaunchSpec,
    pub namespace_plan: NamespaceCompilationPlan,
    pub catalog: GeneratedNodeCatalog,
    pub devices: Vec<CompiledDeviceDefinition>,
    pub control: SessionControlConfig,
    pub readiness_timeout_ms: Option<u64>,
}

impl CompiledOpcUaSession {
    /// Runtime extensions currently remain empty for OPC UA.
    pub fn runtime_extensions(&self) -> RuntimeExtensions {
        RuntimeExtensions::default()
    }
}

/// Serialized launch payload consumed by the runtime driver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpcUaCompiledLaunchConfig {
    pub session_name: String,
    pub server_config: OpcUaServerConfig,
    pub catalog: GeneratedNodeCatalog,
    pub devices: Vec<CompiledDeviceDefinition>,
    pub control: SessionControlConfig,
    pub readiness_timeout_ms: Option<u64>,
}

/// Compiled device bundle for runtime registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledDeviceDefinition {
    pub device_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Tags::is_empty")]
    pub tags: Tags,
    #[serde(default)]
    pub points: Vec<CompiledPointBinding>,
}

/// Stable point binding metadata compiled into a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledPointBinding {
    pub device_id: String,
    pub point_id: String,
    pub node_id: NodeId,
    pub node_class: String,
    pub display_name: String,
    pub browse_name: String,
    pub writable: bool,
    pub historizing: bool,
    pub sampling_interval_ms: Option<u32>,
    pub data_type: DataType,
    pub point_def: DataPointDef,
    #[serde(default, skip_serializing_if = "Tags::is_empty")]
    pub tags: Tags,
}

/// Compiled reference entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledNodeReference {
    pub source_node_id: NodeId,
    pub reference_type: ReferenceTypeId,
    pub target_node_id: NodeId,
    pub direction: ReferenceDirection,
}

impl CompiledNodeReference {
    fn as_reference(&self) -> Reference {
        match self.direction {
            ReferenceDirection::Forward => Reference::forward(
                self.source_node_id.clone(),
                self.reference_type,
                self.target_node_id.clone(),
            ),
            ReferenceDirection::Inverse => Reference::inverse(
                self.source_node_id.clone(),
                self.reference_type,
                self.target_node_id.clone(),
            ),
        }
    }

    fn remap_namespaces(&self, mapping: &BTreeMap<u16, u16>) -> OpcUaResult<Self> {
        Ok(Self {
            source_node_id: remap_node_id(&self.source_node_id, mapping)?,
            reference_type: self.reference_type,
            target_node_id: remap_node_id(&self.target_node_id, mapping)?,
            direction: self.direction,
        })
    }
}

/// Type-tree seed metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeTreeSeed {
    pub namespace_uri: String,
}

/// Canonical compiled node representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GeneratedNodeDefinition {
    Object {
        node_id: NodeId,
        browse_name: QualifiedName,
        display_name: LocalizedText,
        description: Option<LocalizedText>,
        event_notifier: u8,
        folder_like: bool,
    },
    Variable {
        node_id: NodeId,
        browse_name: QualifiedName,
        display_name: LocalizedText,
        description: Option<LocalizedText>,
        data_type: NodeId,
        value: Value,
        writable: bool,
        historizing: bool,
        sampling_interval_ms: Option<u32>,
    },
    Method {
        node_id: NodeId,
        browse_name: QualifiedName,
        display_name: LocalizedText,
        description: Option<LocalizedText>,
        executable: bool,
    },
    ObjectType {
        node_id: NodeId,
        browse_name: QualifiedName,
        display_name: LocalizedText,
    },
    VariableType {
        node_id: NodeId,
        browse_name: QualifiedName,
        display_name: LocalizedText,
        data_type: NodeId,
    },
    ReferenceType {
        node_id: NodeId,
        browse_name: QualifiedName,
        display_name: LocalizedText,
    },
    DataType {
        node_id: NodeId,
        browse_name: QualifiedName,
        display_name: LocalizedText,
    },
    View {
        node_id: NodeId,
        browse_name: QualifiedName,
        display_name: LocalizedText,
    },
}

impl GeneratedNodeDefinition {
    pub fn node_id(&self) -> &NodeId {
        match self {
            Self::Object { node_id, .. }
            | Self::Variable { node_id, .. }
            | Self::Method { node_id, .. }
            | Self::ObjectType { node_id, .. }
            | Self::VariableType { node_id, .. }
            | Self::ReferenceType { node_id, .. }
            | Self::DataType { node_id, .. }
            | Self::View { node_id, .. } => node_id,
        }
    }

    pub fn browse_name(&self) -> &QualifiedName {
        match self {
            Self::Object { browse_name, .. }
            | Self::Variable { browse_name, .. }
            | Self::Method { browse_name, .. }
            | Self::ObjectType { browse_name, .. }
            | Self::VariableType { browse_name, .. }
            | Self::ReferenceType { browse_name, .. }
            | Self::DataType { browse_name, .. }
            | Self::View { browse_name, .. } => browse_name,
        }
    }

    pub fn display_name(&self) -> &LocalizedText {
        match self {
            Self::Object { display_name, .. }
            | Self::Variable { display_name, .. }
            | Self::Method { display_name, .. }
            | Self::ObjectType { display_name, .. }
            | Self::VariableType { display_name, .. }
            | Self::ReferenceType { display_name, .. }
            | Self::DataType { display_name, .. }
            | Self::View { display_name, .. } => display_name,
        }
    }

    pub fn node_class_name(&self) -> &'static str {
        match self {
            Self::Object {
                folder_like: true, ..
            } => "folder",
            Self::Object { .. } => "object",
            Self::Variable { .. } => "variable",
            Self::Method { .. } => "method",
            Self::ObjectType { .. } => "object_type",
            Self::VariableType { .. } => "variable_type",
            Self::ReferenceType { .. } => "reference_type",
            Self::DataType { .. } => "data_type",
            Self::View { .. } => "view",
        }
    }

    pub fn is_variable(&self) -> bool {
        matches!(self, Self::Variable { .. })
    }

    fn remap_namespaces(&self, mapping: &BTreeMap<u16, u16>) -> OpcUaResult<Self> {
        Ok(match self {
            Self::Object {
                node_id,
                browse_name,
                display_name,
                description,
                event_notifier,
                folder_like,
            } => Self::Object {
                node_id: remap_node_id(node_id, mapping)?,
                browse_name: remap_qualified_name(browse_name, mapping),
                display_name: display_name.clone(),
                description: description.clone(),
                event_notifier: *event_notifier,
                folder_like: *folder_like,
            },
            Self::Variable {
                node_id,
                browse_name,
                display_name,
                description,
                data_type,
                value,
                writable,
                historizing,
                sampling_interval_ms,
            } => Self::Variable {
                node_id: remap_node_id(node_id, mapping)?,
                browse_name: remap_qualified_name(browse_name, mapping),
                display_name: display_name.clone(),
                description: description.clone(),
                data_type: remap_node_id(data_type, mapping)?,
                value: value.clone(),
                writable: *writable,
                historizing: *historizing,
                sampling_interval_ms: *sampling_interval_ms,
            },
            Self::Method {
                node_id,
                browse_name,
                display_name,
                description,
                executable,
            } => Self::Method {
                node_id: remap_node_id(node_id, mapping)?,
                browse_name: remap_qualified_name(browse_name, mapping),
                display_name: display_name.clone(),
                description: description.clone(),
                executable: *executable,
            },
            Self::ObjectType {
                node_id,
                browse_name,
                display_name,
            } => Self::ObjectType {
                node_id: remap_node_id(node_id, mapping)?,
                browse_name: remap_qualified_name(browse_name, mapping),
                display_name: display_name.clone(),
            },
            Self::VariableType {
                node_id,
                browse_name,
                display_name,
                data_type,
            } => Self::VariableType {
                node_id: remap_node_id(node_id, mapping)?,
                browse_name: remap_qualified_name(browse_name, mapping),
                display_name: display_name.clone(),
                data_type: remap_node_id(data_type, mapping)?,
            },
            Self::ReferenceType {
                node_id,
                browse_name,
                display_name,
            } => Self::ReferenceType {
                node_id: remap_node_id(node_id, mapping)?,
                browse_name: remap_qualified_name(browse_name, mapping),
                display_name: display_name.clone(),
            },
            Self::DataType {
                node_id,
                browse_name,
                display_name,
            } => Self::DataType {
                node_id: remap_node_id(node_id, mapping)?,
                browse_name: remap_qualified_name(browse_name, mapping),
                display_name: display_name.clone(),
            },
            Self::View {
                node_id,
                browse_name,
                display_name,
            } => Self::View {
                node_id: remap_node_id(node_id, mapping)?,
                browse_name: remap_qualified_name(browse_name, mapping),
                display_name: display_name.clone(),
            },
        })
    }

    fn insert_into(&self, address_space: &AddressSpace) {
        match self {
            Self::Object {
                node_id,
                browse_name,
                display_name,
                description,
                event_notifier,
                ..
            } => {
                let mut node =
                    ObjectNode::new(node_id.clone(), browse_name.clone(), display_name.clone())
                        .with_event_notifier(*event_notifier);
                if let Some(description) = description {
                    node = node.with_description(description.clone());
                }
                address_space.insert_node(node);
            }
            Self::Variable {
                node_id,
                browse_name,
                display_name,
                description,
                data_type,
                value,
                writable,
                historizing,
                sampling_interval_ms,
            } => {
                let mut node = VariableNode::new(
                    node_id.clone(),
                    browse_name.clone(),
                    display_name.clone(),
                    data_type.clone(),
                    Variant::from(value.clone()),
                );
                if let Some(description) = description {
                    node = node.with_description(description.clone());
                }
                if *writable {
                    node = node.writable();
                }
                node = node.with_historizing(*historizing);
                if let Some(sampling_interval_ms) = sampling_interval_ms {
                    node = node.with_minimum_sampling_interval(*sampling_interval_ms as f64);
                }
                address_space.insert_node(node);
            }
            Self::Method {
                node_id,
                browse_name,
                display_name,
                description,
                executable,
            } => {
                let mut node =
                    MethodNode::new(node_id.clone(), browse_name.clone(), display_name.clone())
                        .with_executable(*executable);
                if let Some(description) = description {
                    node = node.with_description(description.clone());
                }
                address_space.insert_node(node);
            }
            Self::ObjectType {
                node_id,
                browse_name,
                display_name,
            } => {
                address_space.insert_node(ObjectTypeNode::new(
                    node_id.clone(),
                    browse_name.clone(),
                    display_name.clone(),
                ));
            }
            Self::VariableType {
                node_id,
                browse_name,
                display_name,
                data_type,
            } => {
                address_space.insert_node(VariableTypeNode::new(
                    node_id.clone(),
                    browse_name.clone(),
                    display_name.clone(),
                    data_type.clone(),
                ));
            }
            Self::ReferenceType {
                node_id,
                browse_name,
                display_name,
            } => {
                address_space.insert_node(ReferenceTypeNode::new(
                    node_id.clone(),
                    browse_name.clone(),
                    display_name.clone(),
                ));
            }
            Self::DataType {
                node_id,
                browse_name,
                display_name,
            } => {
                address_space.insert_node(DataTypeNode::new(
                    node_id.clone(),
                    browse_name.clone(),
                    display_name.clone(),
                ));
            }
            Self::View {
                node_id,
                browse_name,
                display_name,
            } => {
                address_space.insert_node(ViewNode::new(
                    node_id.clone(),
                    browse_name.clone(),
                    display_name.clone(),
                ));
            }
        }
    }
}

/// CLI-oriented schema inspection surface.
#[derive(Debug, Clone, Serialize)]
pub struct OpcUaSchemaSummary {
    pub kind: &'static str,
    pub formats: Vec<&'static str>,
    pub top_level_sections: Vec<SchemaSection>,
    pub commands: Vec<&'static str>,
    pub notes: Vec<&'static str>,
}

/// One section in the canonical schema surface.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaSection {
    pub name: &'static str,
    pub required: bool,
    pub purpose: &'static str,
}

impl SchemaSection {
    fn new(name: &'static str, required: bool, purpose: &'static str) -> Self {
        Self {
            name,
            required,
            purpose,
        }
    }
}

/// Inspection summary for a parsed simulator config.
#[derive(Debug, Clone, Serialize)]
pub struct OpcUaConfigSummary {
    pub transports: Vec<String>,
    pub nodesets: Vec<String>,
    pub models: Vec<String>,
    pub devices: Vec<String>,
    pub sessions: Vec<OpcUaSessionSummary>,
    pub presets: Vec<String>,
}

/// Summary of one named session.
#[derive(Debug, Clone, Serialize)]
pub struct OpcUaSessionSummary {
    pub name: String,
    pub transport: String,
    pub models: Vec<String>,
    pub devices: Vec<String>,
    pub preset: Option<String>,
    pub service_name: Option<String>,
}

fn build_server_config(
    defaults: &SimulatorDefaults,
    session: &SessionDefinition,
    transport: &TransportDefinition,
) -> OpcUaServerConfig {
    let server_name = transport
        .server_name
        .clone()
        .or_else(|| session.service_name.clone())
        .unwrap_or_else(|| defaults.server_name.clone());
    let endpoint_url = format!(
        "opc.tcp://{}:{}{}",
        transport.bind, transport.port, transport.endpoint_path
    );
    let mut config = OpcUaServerConfig {
        endpoint_url,
        server_name,
        security_policy: transport
            .security_profile
            .clone()
            .or_else(|| defaults.security_profile.clone())
            .unwrap_or_else(|| "None".into()),
        ..Default::default()
    };
    if let Some(min_publishing_interval_ms) = defaults.min_publishing_interval_ms {
        config.min_publishing_interval_ms = min_publishing_interval_ms;
    }
    config
}

fn compile_device(
    device_name: &str,
    device: &DeviceDefinition,
    node_map: &BTreeMap<String, GeneratedNodeDefinition>,
) -> OpcUaResult<CompiledDeviceDefinition> {
    let mut bindings = if device.node_bindings.is_empty() {
        auto_bind_model(device_name, device, node_map)?
    } else {
        let mut bindings = Vec::new();
        for binding in &device.node_bindings {
            bindings.push(compile_binding(device_name, device, binding, node_map)?);
        }
        bindings
    };

    bindings.sort_by(|left, right| left.point_id.cmp(&right.point_id));

    Ok(CompiledDeviceDefinition {
        device_id: device_name.to_string(),
        name: device
            .name
            .clone()
            .unwrap_or_else(|| device_name.replace('-', " ")),
        tags: device.tags.clone(),
        points: bindings,
    })
}

fn auto_bind_model(
    device_name: &str,
    device: &DeviceDefinition,
    node_map: &BTreeMap<String, GeneratedNodeDefinition>,
) -> OpcUaResult<Vec<CompiledPointBinding>> {
    let mut bindings = Vec::new();
    for node in node_map.values() {
        if let GeneratedNodeDefinition::Variable {
            node_id,
            display_name,
            browse_name,
            writable,
            historizing,
            sampling_interval_ms,
            data_type,
            ..
        } = node
        {
            let point_id = node_id.to_string();
            bindings.push(compiled_binding_from_node(
                device_name,
                &device.tags,
                &point_id,
                node,
                *writable,
                *historizing,
                *sampling_interval_ms,
                None,
                browse_name.name.as_str(),
                display_name.text.as_str(),
                data_type,
            )?);
        }
    }
    Ok(bindings)
}

fn compile_binding(
    device_name: &str,
    device: &DeviceDefinition,
    binding: &NodeBindingDefinition,
    node_map: &BTreeMap<String, GeneratedNodeDefinition>,
) -> OpcUaResult<CompiledPointBinding> {
    let node = node_map.get(&binding.node_id).ok_or_else(|| {
        OpcUaError::Config(format!(
            "device '{}' references unknown node '{}'",
            device_name, binding.node_id
        ))
    })?;
    let GeneratedNodeDefinition::Variable {
        browse_name,
        display_name,
        writable,
        historizing,
        sampling_interval_ms,
        data_type,
        ..
    } = node
    else {
        return Err(OpcUaError::Config(format!(
            "device '{}' binding '{}' targets non-variable node '{}'",
            device_name, binding.point_id, binding.node_id
        )));
    };
    let merged_tags = device.tags.clone().merged_with(binding.tags.clone());
    compiled_binding_from_node(
        device_name,
        &merged_tags,
        &binding.point_id,
        node,
        binding.writable.unwrap_or(*writable),
        binding.historizing.unwrap_or(*historizing),
        binding.sampling_interval_ms.or(*sampling_interval_ms),
        binding.seed.clone(),
        binding
            .label
            .as_deref()
            .unwrap_or(browse_name.name.as_str()),
        display_name.text.as_str(),
        data_type,
    )
}

fn compiled_binding_from_node(
    device_name: &str,
    tags: &Tags,
    point_id: &str,
    node: &GeneratedNodeDefinition,
    writable: bool,
    historizing: bool,
    sampling_interval_ms: Option<u32>,
    seed: Option<Value>,
    label: &str,
    display_name: &str,
    data_type: &NodeId,
) -> OpcUaResult<CompiledPointBinding> {
    let node_id = node.node_id().clone();
    let data_type_kind = map_data_type(data_type);
    let access = if writable {
        AccessMode::ReadWrite
    } else {
        AccessMode::ReadOnly
    };
    let mut point_def = DataPointDef::new(point_id, label, data_type_kind)
        .with_access(access)
        .with_address(Address::OpcUa {
            node_id: node_id.to_string(),
        });
    if let Some(seed) = seed {
        point_def.default_value = Some(seed);
    }
    if !matches!(node, GeneratedNodeDefinition::Variable { .. }) {
        return Err(OpcUaError::Config(format!(
            "point '{}' must bind to a variable node",
            point_id
        )));
    }
    Ok(CompiledPointBinding {
        device_id: device_name.to_string(),
        point_id: point_id.to_string(),
        node_id,
        node_class: node.node_class_name().to_string(),
        display_name: display_name.to_string(),
        browse_name: node.browse_name().name.clone(),
        writable,
        historizing,
        sampling_interval_ms,
        data_type: data_type_kind,
        point_def,
        tags: tags.clone(),
    })
}

fn map_data_type(data_type: &NodeId) -> DataType {
    match data_type.as_numeric() {
        Some(1) => DataType::Bool,
        Some(2) => DataType::Int8,
        Some(3) => DataType::UInt8,
        Some(4) => DataType::Int16,
        Some(5) => DataType::UInt16,
        Some(6) => DataType::Int32,
        Some(7) => DataType::UInt32,
        Some(8) => DataType::Int64,
        Some(9) => DataType::UInt64,
        Some(10) => DataType::Float32,
        Some(11) => DataType::Float64,
        Some(12) => DataType::String,
        Some(13) => DataType::DateTime,
        Some(15) => DataType::ByteString,
        _ => DataType::Float64,
    }
}

#[derive(Debug, Clone)]
struct ImportedNodeSet {
    local_namespace_table: Vec<String>,
    nodes: Vec<GeneratedNodeDefinition>,
    references: Vec<CompiledNodeReference>,
}

fn import_nodeset_source(
    source: &NodeSetSource,
    base_path: Option<&Path>,
) -> OpcUaResult<ImportedNodeSet> {
    match source {
        NodeSetSource::File {
            path,
            namespace_uri_override,
        } => {
            let resolved = resolve_path(base_path, path);
            let xml = fs::read_to_string(&resolved)?;
            import_nodeset_xml(&xml, namespace_uri_override.as_ref())
        }
        NodeSetSource::Embedded {
            alias,
            namespace_uri_override,
        } => import_embedded_nodeset(alias, namespace_uri_override.as_ref()),
    }
}

fn validate_nodeset_source(
    name: &str,
    source: &NodeSetSource,
    base_path: Option<&Path>,
) -> OpcUaResult<()> {
    match source {
        NodeSetSource::File { path, .. } => {
            let resolved = resolve_path(base_path, path);
            if !resolved.exists() {
                return Err(OpcUaError::Config(format!(
                    "nodeset '{}' path '{}' does not exist",
                    name,
                    resolved.display()
                )));
            }
        }
        NodeSetSource::Embedded { alias, .. } => {
            if !matches!(alias.as_str(), "minimal" | "demo" | "base_simulation") {
                return Err(OpcUaError::Config(format!(
                    "nodeset '{}' references unsupported embedded alias '{}'",
                    name, alias
                )));
            }
        }
    }
    Ok(())
}

fn import_embedded_nodeset(
    alias: &str,
    namespace_uri_override: Option<&String>,
) -> OpcUaResult<ImportedNodeSet> {
    let namespace_uri = namespace_uri_override
        .cloned()
        .unwrap_or_else(|| format!("urn:mabinogion:opcua:embedded:{}", alias));
    match alias {
        "minimal" | "demo" | "base_simulation" => Ok(ImportedNodeSet {
            local_namespace_table: vec![STANDARD_NAMESPACE_URI.to_string(), namespace_uri],
            nodes: vec![
                GeneratedNodeDefinition::Object {
                    node_id: NodeId::string(1, format!("embedded/{}", alias)),
                    browse_name: QualifiedName::new(1, format!("Embedded{}", alias)),
                    display_name: localized_text(&format!("Embedded {}", alias)),
                    description: Some(localized_text("Embedded NodeSet2 alias")),
                    event_notifier: 0,
                    folder_like: false,
                },
                GeneratedNodeDefinition::Variable {
                    node_id: NodeId::string(1, format!("embedded/{}/value", alias)),
                    browse_name: QualifiedName::new(1, "Value"),
                    display_name: localized_text("Value"),
                    description: Some(localized_text("Embedded demo variable")),
                    data_type: NodeId::numeric(0, 11),
                    value: Value::F64(0.0),
                    writable: true,
                    historizing: false,
                    sampling_interval_ms: Some(100),
                },
            ],
            references: vec![CompiledNodeReference {
                source_node_id: NodeId::string(1, format!("embedded/{}", alias)),
                reference_type: ReferenceTypeId::HasComponent,
                target_node_id: NodeId::string(1, format!("embedded/{}/value", alias)),
                direction: ReferenceDirection::Forward,
            }],
        }),
        _ => Err(OpcUaError::Config(format!(
            "unsupported embedded NodeSet alias '{}'",
            alias
        ))),
    }
}

fn import_nodeset_xml(
    xml: &str,
    namespace_uri_override: Option<&String>,
) -> OpcUaResult<ImportedNodeSet> {
    let namespace_uris = parse_namespace_uris(xml, namespace_uri_override);
    let mut nodes = Vec::new();
    let mut references = Vec::new();

    for tag in [
        "UAObject",
        "UAVariable",
        "UAMethod",
        "UAObjectType",
        "UAVariableType",
        "UAReferenceType",
        "UADataType",
        "UAView",
    ] {
        for element in collect_xml_elements(xml, tag) {
            let (node, node_references) = parse_nodeset_element(tag, &element)?;
            nodes.push(node);
            references.extend(node_references);
        }
    }

    Ok(ImportedNodeSet {
        local_namespace_table: namespace_uris,
        nodes,
        references,
    })
}

fn parse_nodeset_element(
    tag: &str,
    element: &XmlElement,
) -> OpcUaResult<(GeneratedNodeDefinition, Vec<CompiledNodeReference>)> {
    let node_id = parse_node_id(required_attr(element, "NodeId")?)?;
    let browse_name = parse_browse_name(required_attr(element, "BrowseName")?);
    let display_name = localized_text(
        element
            .text_of("DisplayName")
            .unwrap_or(browse_name.name.clone()),
    );
    let description = element.text_of("Description").map(localized_text_owned);
    let references = parse_reference_elements(node_id.clone(), element)?;

    let node = match tag {
        "UAObject" => GeneratedNodeDefinition::Object {
            node_id,
            browse_name,
            display_name,
            description,
            event_notifier: parse_u8_attr(element.attr("EventNotifier")).unwrap_or(0),
            folder_like: false,
        },
        "UAVariable" => GeneratedNodeDefinition::Variable {
            node_id,
            browse_name,
            display_name,
            description,
            data_type: element
                .attr("DataType")
                .map(parse_node_id)
                .transpose()?
                .unwrap_or_else(|| NodeId::numeric(0, 11)),
            value: parse_value_element(element.value_xml()).unwrap_or(Value::Null),
            writable: parse_access_level(element.attr("AccessLevel")).can_write(),
            historizing: parse_bool_attr(element.attr("Historizing")).unwrap_or(false),
            sampling_interval_ms: parse_f64_attr(element.attr("MinimumSamplingInterval"))
                .map(|value| value.max(0.0) as u32),
        },
        "UAMethod" => GeneratedNodeDefinition::Method {
            node_id,
            browse_name,
            display_name,
            description,
            executable: parse_bool_attr(element.attr("Executable")).unwrap_or(true),
        },
        "UAObjectType" => GeneratedNodeDefinition::ObjectType {
            node_id,
            browse_name,
            display_name,
        },
        "UAVariableType" => GeneratedNodeDefinition::VariableType {
            node_id,
            browse_name,
            display_name,
            data_type: element
                .attr("DataType")
                .map(parse_node_id)
                .transpose()?
                .unwrap_or_else(|| NodeId::numeric(0, 11)),
        },
        "UAReferenceType" => GeneratedNodeDefinition::ReferenceType {
            node_id,
            browse_name,
            display_name,
        },
        "UADataType" => GeneratedNodeDefinition::DataType {
            node_id,
            browse_name,
            display_name,
        },
        "UAView" => GeneratedNodeDefinition::View {
            node_id,
            browse_name,
            display_name,
        },
        other => {
            return Err(OpcUaError::Config(format!(
                "unsupported NodeSet element '{}'",
                other
            )))
        }
    };

    Ok((node, references))
}

fn parse_reference_elements(
    source_node_id: NodeId,
    element: &XmlElement,
) -> OpcUaResult<Vec<CompiledNodeReference>> {
    let Some(references_block) = element.child_xml("References") else {
        return Ok(Vec::new());
    };
    let mut references = Vec::new();
    for reference in collect_xml_elements(references_block, "Reference") {
        let reference_type = parse_reference_type(required_attr(&reference, "ReferenceType")?)?;
        let target_node_id = parse_node_id(reference.inner.trim())?;
        let direction = if reference.attr("IsForward") == Some("false") {
            ReferenceDirection::Inverse
        } else {
            ReferenceDirection::Forward
        };
        references.push(CompiledNodeReference {
            source_node_id: source_node_id.clone(),
            reference_type,
            target_node_id,
            direction,
        });
    }
    Ok(references)
}

fn parse_namespace_uris(xml: &str, override_uri: Option<&String>) -> Vec<String> {
    let mut table = vec![STANDARD_NAMESPACE_URI.to_string()];
    if let Some(block) = child_xml_block(xml, "NamespaceUris") {
        for element in collect_xml_elements(block, "Uri") {
            let value = element.inner.trim();
            if !value.is_empty() {
                table.push(value.to_string());
            }
        }
    }
    if let Some(override_uri) = override_uri {
        if table.len() == 1 {
            table.push(override_uri.clone());
        } else if let Some(slot) = table.get_mut(1) {
            *slot = override_uri.clone();
        }
    }
    table
}

fn collect_type_tree_seeds(namespace_table: &[String]) -> Vec<TypeTreeSeed> {
    namespace_table
        .iter()
        .cloned()
        .map(|namespace_uri| TypeTreeSeed { namespace_uri })
        .collect()
}

fn namespace_mapping(local: &[String], global: &[String]) -> OpcUaResult<BTreeMap<u16, u16>> {
    let mut mapping = BTreeMap::new();
    for (index, uri) in local.iter().enumerate() {
        let global_index = global
            .iter()
            .position(|candidate| candidate == uri)
            .ok_or_else(|| {
                OpcUaError::Config(format!(
                    "missing namespace URI '{}' during compilation",
                    uri
                ))
            })?;
        mapping.insert(index as u16, global_index as u16);
    }
    Ok(mapping)
}

fn remap_node_id(node_id: &NodeId, mapping: &BTreeMap<u16, u16>) -> OpcUaResult<NodeId> {
    let namespace = mapping
        .get(&node_id.namespace())
        .copied()
        .unwrap_or(node_id.namespace());
    Ok(match node_id.identifier() {
        NodeIdType::Numeric(value) => NodeId::numeric(namespace, *value),
        NodeIdType::String(value) => NodeId::string(namespace, value.clone()),
        NodeIdType::Guid(value) => NodeId::guid(namespace, *value),
        NodeIdType::ByteString(value) => NodeId::byte_string(namespace, value.clone()),
    })
}

fn remap_qualified_name(
    browse_name: &QualifiedName,
    mapping: &BTreeMap<u16, u16>,
) -> QualifiedName {
    QualifiedName::new(
        mapping
            .get(&browse_name.namespace_index)
            .copied()
            .unwrap_or(browse_name.namespace_index),
        browse_name.name.clone(),
    )
}

fn parse_node_id(value: &str) -> OpcUaResult<NodeId> {
    value
        .parse::<NodeId>()
        .map_err(|error| OpcUaError::InvalidNodeId(error.to_string()))
}

fn parse_browse_name(value: &str) -> QualifiedName {
    if let Some((namespace, name)) = value.split_once(':') {
        if let Ok(namespace_index) = namespace.parse::<u16>() {
            return QualifiedName::new(namespace_index, name);
        }
    }
    QualifiedName::new(0, value)
}

fn parse_reference_type(value: &str) -> OpcUaResult<ReferenceTypeId> {
    if let Ok(node_id) = parse_node_id(value) {
        if let Some(reference_type) = ReferenceTypeId::from_node_id(&node_id) {
            return Ok(reference_type);
        }
    }
    match value {
        "Organizes" => Ok(ReferenceTypeId::Organizes),
        "HasComponent" => Ok(ReferenceTypeId::HasComponent),
        "HasProperty" => Ok(ReferenceTypeId::HasProperty),
        "HasSubtype" => Ok(ReferenceTypeId::HasSubtype),
        "HasTypeDefinition" => Ok(ReferenceTypeId::HasTypeDefinition),
        "HasNotifier" => Ok(ReferenceTypeId::HasNotifier),
        "HasEventSource" => Ok(ReferenceTypeId::HasEventSource),
        other => Err(OpcUaError::Config(format!(
            "unsupported reference type '{}'",
            other
        ))),
    }
}

fn parse_value_element(xml: Option<&str>) -> Option<Value> {
    let value_xml = xml?;
    for tag in [
        "Boolean",
        "Double",
        "Float",
        "Int16",
        "UInt16",
        "Int32",
        "UInt32",
        "Int64",
        "UInt64",
        "String",
        "DateTime",
        "ByteString",
    ] {
        if let Some(element) = collect_xml_elements(value_xml, tag).into_iter().next() {
            let raw = element.inner.trim();
            return match tag {
                "Boolean" => raw.parse::<bool>().ok().map(Value::Bool),
                "Double" | "Float" => raw.parse::<f64>().ok().map(Value::F64),
                "Int16" | "Int32" | "Int64" => raw.parse::<i64>().ok().map(Value::I64),
                "UInt16" | "UInt32" | "UInt64" => raw.parse::<u64>().ok().map(Value::U64),
                "String" => Some(Value::String(raw.to_string())),
                "DateTime" => Some(Value::String(raw.to_string())),
                "ByteString" => Some(Value::Bytes(raw.as_bytes().to_vec())),
                _ => None,
            };
        }
    }
    None
}

fn parse_access_level(value: Option<&str>) -> AccessLevel {
    value
        .and_then(|raw| raw.parse::<u8>().ok())
        .map(AccessLevel::from_raw)
        .unwrap_or(AccessLevel::CURRENT_READ)
}

fn parse_bool_attr(value: Option<&str>) -> Option<bool> {
    value.map(|value| value.eq_ignore_ascii_case("true") || value == "1")
}

fn parse_u8_attr(value: Option<&str>) -> Option<u8> {
    value.and_then(|value| value.parse::<u8>().ok())
}

fn parse_f64_attr(value: Option<&str>) -> Option<f64> {
    value.and_then(|value| value.parse::<f64>().ok())
}

fn localized_text(text: impl Into<String>) -> LocalizedText {
    LocalizedText::new("en-US", text.into())
}

fn localized_text_owned(text: impl Into<String>) -> LocalizedText {
    LocalizedText::new("en-US", text.into())
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn resolve_path(base_path: Option<&Path>, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match base_path.and_then(Path::parent) {
        Some(parent) => parent.join(path),
        None => path.to_path_buf(),
    }
}

fn required_attr<'a>(element: &'a XmlElement, name: &str) -> OpcUaResult<&'a str> {
    element.attr(name).ok_or_else(|| {
        OpcUaError::Config(format!(
            "NodeSet element '{}' is missing required attribute '{}'",
            element.tag, name
        ))
    })
}

fn is_standard_node(node_id: &NodeId) -> bool {
    node_id.namespace() == 0
}

fn default_namespace_uri() -> String {
    DEFAULT_NAMESPACE_URI.to_string()
}

fn default_server_name() -> String {
    DEFAULT_SERVER_NAME.to_string()
}

fn default_bind_address() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    4840
}

fn default_endpoint_path() -> String {
    DEFAULT_ENDPOINT_PATH.to_string()
}

fn default_preset_nodes() -> usize {
    100
}

fn default_preset_base_node() -> u32 {
    1000
}

fn default_true() -> bool {
    true
}

fn default_reference_direction() -> ReferenceDirection {
    ReferenceDirection::Forward
}

#[derive(Debug, Clone)]
struct XmlElement {
    tag: String,
    attrs: BTreeMap<String, String>,
    inner: String,
}

impl XmlElement {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(String::as_str)
    }

    fn text_of(&self, tag: &str) -> Option<String> {
        collect_xml_elements(&self.inner, tag)
            .into_iter()
            .next()
            .map(|element| element.inner.trim().to_string())
    }

    fn child_xml(&self, tag: &str) -> Option<&str> {
        child_xml_block(&self.inner, tag)
    }

    fn value_xml(&self) -> Option<&str> {
        self.child_xml("Value")
    }
}

fn collect_xml_elements(xml: &str, tag: &str) -> Vec<XmlElement> {
    let mut elements = Vec::new();
    let mut position = 0;
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);

    while let Some(relative_start) = xml[position..].find(&open) {
        let start = position + relative_start;
        let Some(relative_end) = xml[start..].find('>') else {
            break;
        };
        let end = start + relative_end;
        let open_tag = &xml[start + 1..end];
        let self_closing = open_tag.trim_end().ends_with('/');
        let attrs = parse_xml_attributes(
            open_tag
                .strip_prefix(tag)
                .unwrap_or(open_tag)
                .trim()
                .trim_end_matches('/'),
        );

        if self_closing {
            elements.push(XmlElement {
                tag: tag.to_string(),
                attrs,
                inner: String::new(),
            });
            position = end + 1;
            continue;
        }

        let search_start = end + 1;
        let Some(relative_close_start) = xml[search_start..].find(&close) else {
            break;
        };
        let close_start = search_start + relative_close_start;
        elements.push(XmlElement {
            tag: tag.to_string(),
            attrs,
            inner: xml[search_start..close_start].to_string(),
        });
        position = close_start + close.len();
    }

    elements
}

fn child_xml_block<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(&xml[start..end])
}

fn parse_xml_attributes(input: &str) -> BTreeMap<String, String> {
    let mut attrs = BTreeMap::new();
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        let key_start = index;
        while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'=' {
            index += 1;
        }
        let key = input[key_start..index].trim();
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b'=') {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'"' {
            break;
        }
        index += 1;
        let value_start = index;
        while index < bytes.len() && bytes[index] != b'"' {
            index += 1;
        }
        if index <= bytes.len() {
            attrs.insert(key.to_string(), input[value_start..index].to_string());
        }
        index += 1;
    }
    attrs
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::NamedTempFile;

    fn sample_nodeset() -> String {
        r#"
        <UANodeSet>
          <NamespaceUris>
            <Uri>urn:example:model</Uri>
          </NamespaceUris>
          <UAObject NodeId="ns=1;s=Machine" BrowseName="1:Machine">
            <DisplayName>Machine</DisplayName>
            <References>
              <Reference ReferenceType="Organizes">i=85</Reference>
            </References>
          </UAObject>
          <UAVariable NodeId="ns=1;s=Machine.Temperature" BrowseName="1:Temperature" DataType="i=11" AccessLevel="3" Historizing="true">
            <DisplayName>Temperature</DisplayName>
            <Value><Double>21.5</Double></Value>
            <References>
              <Reference ReferenceType="HasComponent">ns=1;s=Machine</Reference>
            </References>
          </UAVariable>
        </UANodeSet>
        "#
        .to_string()
    }

    #[test]
    fn schema_summary_lists_canonical_sections() {
        let summary = schema_summary();
        assert_eq!(summary.kind, "opcua_simulator");
        assert!(summary
            .top_level_sections
            .iter()
            .any(|section| section.name == "nodesets"));
    }

    #[test]
    fn imported_nodeset_parses_nodes_and_references() {
        let imported = import_nodeset_xml(&sample_nodeset(), None).unwrap();
        assert_eq!(imported.local_namespace_table.len(), 2);
        assert_eq!(imported.nodes.len(), 2);
        assert_eq!(imported.references.len(), 2);
    }

    #[test]
    fn config_compiles_file_backed_session() {
        let nodeset = NamedTempFile::new().unwrap();
        fs::write(nodeset.path(), sample_nodeset()).unwrap();

        let config = OpcUaSimulatorConfig {
            transports: BTreeMap::from([(
                "main".into(),
                TransportDefinition {
                    bind: "127.0.0.1".into(),
                    port: 4840,
                    endpoint_path: "/sim".into(),
                    security_profile: Some("None".into()),
                    server_name: None,
                },
            )]),
            nodesets: BTreeMap::from([(
                "demo".into(),
                NodeSetSource::File {
                    path: nodeset.path().to_path_buf(),
                    namespace_uri_override: None,
                },
            )]),
            models: BTreeMap::from([(
                "machine".into(),
                ModelDefinition {
                    nodesets: vec!["demo".into()],
                    ..Default::default()
                },
            )]),
            sessions: BTreeMap::from([(
                "demo".into(),
                SessionDefinition {
                    transport: "main".into(),
                    models: vec!["machine".into()],
                    devices: Vec::new(),
                    preset: None,
                    service_name: Some("opcua-demo".into()),
                    readiness_timeout_ms: Some(1_000),
                    control: SessionControlConfig::default(),
                },
            )]),
            ..Default::default()
        };

        let compiled = config
            .compile_session("demo", Some(nodeset.path()))
            .unwrap();
        assert_eq!(compiled.session_name, "demo");
        assert!(!compiled.catalog.nodes.is_empty());
        assert!(!compiled.devices.is_empty());
        assert_eq!(compiled.launch.protocol, "opcua");
    }

    #[test]
    fn legacy_preset_generates_ephemeral_model() {
        let config = OpcUaSimulatorConfig {
            transports: BTreeMap::from([("main".into(), TransportDefinition::default())]),
            presets: BTreeMap::from([("legacy".into(), PresetDefinition::default())]),
            sessions: BTreeMap::from([(
                "legacy".into(),
                SessionDefinition {
                    transport: "main".into(),
                    preset: Some("legacy".into()),
                    service_name: Some("legacy-service".into()),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };

        let compiled = config.compile_session("legacy", None).unwrap();
        assert!(!compiled.catalog.nodes.is_empty());
        assert!(!compiled.devices.is_empty());
    }
}
