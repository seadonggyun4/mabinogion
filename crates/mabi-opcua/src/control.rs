//! In-process control-plane surface for compiled OPC UA simulator sessions.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;

use mabi_core::types::{Address, DataPoint};
use mabi_core::value::Value;
use mabi_runtime::{ProtocolDriverRegistry, RuntimeSession, RuntimeSessionSpec};

use crate::error::{OpcUaError, OpcUaResult};
use crate::modeling::CompiledOpcUaSession;
use crate::sdk::subscription::{
    DurableSubscriptionStatus, DurableSubscriptionStore, SubscriptionDurabilityMode,
};
use crate::security::{SecurityAuditStatus, SecurityManager, SecurityStatus};

/// Lifecycle-oriented control surface for a compiled session.
#[async_trait]
pub trait SessionControlPort: Send {
    async fn status(&self) -> OpcUaResult<SessionStatus>;
    async fn snapshot(&self) -> OpcUaResult<SessionSnapshot>;
    async fn reset(&mut self) -> OpcUaResult<SessionSnapshot>;
}

/// Security admin surface used by CLI commands.
#[async_trait]
pub trait SecurityControlPort: Send {
    async fn security_status(&self) -> OpcUaResult<SecurityControlStatus>;
    async fn trust_reload(&self) -> OpcUaResult<SecurityControlStatus>;
    async fn rotate_server_certificate(
        &self,
        certificate_path: PathBuf,
        private_key_path: PathBuf,
    ) -> OpcUaResult<SecurityControlStatus>;
    async fn audit_summary(&self) -> OpcUaResult<SecurityAuditStatus>;
}

/// Node catalog inspection surface used by CLI commands.
pub trait NodeCatalogPort {
    fn list_nodes(&self) -> OpcUaResult<Vec<NodeDescriptor>>;
}

/// Point and raw-node read/write surface.
#[async_trait]
pub trait NodeValueControlPort {
    async fn read(&self, target: &NodeTarget) -> OpcUaResult<DataPoint>;
    async fn write(&self, target: &NodeTarget, value: Value) -> OpcUaResult<()>;
}

/// Stable session status view.
#[derive(Debug, Clone, Serialize)]
pub struct SessionStatus {
    pub session_name: String,
    pub services: usize,
    pub devices: usize,
    pub nodes: usize,
    pub namespaces: usize,
    pub allow_raw_node_access: bool,
    pub durability_mode: String,
    pub restore_on_start: bool,
    pub persisted_state_present: bool,
    pub restored_subscriptions: usize,
    pub detached_restored_subscriptions: usize,
    pub last_durable_flush_at: Option<String>,
    pub last_durable_flush_result: String,
    pub diagnostics_summary: String,
    pub security_profile: String,
    pub audit_sink: String,
    pub allow_trust_reload: bool,
    pub allow_certificate_rotation: bool,
    pub audit_status: SecurityAuditStatus,
    pub generated_type_entries: usize,
    pub generated_type_module: String,
}

/// Snapshot returned by reset and snapshot operations.
#[derive(Debug, Clone, Serialize)]
pub struct SessionSnapshot {
    pub status: SessionStatus,
    pub services: Vec<mabi_runtime::ServiceSnapshot>,
}

/// Stable security admin view.
#[derive(Debug, Clone, Serialize)]
pub struct SecurityControlStatus {
    pub profile_name: String,
    pub allow_trust_reload: bool,
    pub allow_certificate_rotation: bool,
    pub status: SecurityStatus,
}

/// Operator-facing node catalog record.
#[derive(Debug, Clone, Serialize)]
pub struct NodeDescriptor {
    pub device_id: String,
    pub point_id: String,
    pub node_id: String,
    pub browse_name: String,
    pub display_name: String,
    pub node_class: String,
    pub writable: bool,
    pub historizing: bool,
    pub sampling_interval_ms: Option<u32>,
}

/// Read/write selector used by control commands.
#[derive(Debug, Clone, Default)]
pub struct NodeTarget {
    pub device_id: Option<String>,
    pub point_id: Option<String>,
    pub node_id: Option<String>,
}

/// In-process control session over a compiled OPC UA simulator session.
pub struct OpcUaControlSession {
    registry: ProtocolDriverRegistry,
    compiled: CompiledOpcUaSession,
    fallback_readiness_timeout: Duration,
    runtime_session: RuntimeSession,
    security_manager: SecurityManager,
}

impl OpcUaControlSession {
    pub async fn new(
        registry: ProtocolDriverRegistry,
        compiled: CompiledOpcUaSession,
        fallback_readiness_timeout: Duration,
    ) -> OpcUaResult<Self> {
        let runtime_session =
            Self::start_runtime(&registry, &compiled, fallback_readiness_timeout).await?;
        let security_manager = Self::build_security_manager(&compiled)?;
        Ok(Self {
            registry,
            compiled,
            fallback_readiness_timeout,
            runtime_session,
            security_manager,
        })
    }

    fn build_security_manager(compiled: &CompiledOpcUaSession) -> OpcUaResult<SecurityManager> {
        let manager = SecurityManager::new(compiled.security.manager_config.clone());
        manager
            .initialize()
            .map_err(|error| OpcUaError::Server(error.to_string()))?;
        Ok(manager)
    }

    async fn start_runtime(
        registry: &ProtocolDriverRegistry,
        compiled: &CompiledOpcUaSession,
        fallback_readiness_timeout: Duration,
    ) -> OpcUaResult<RuntimeSession> {
        let session = RuntimeSession::new(
            RuntimeSessionSpec {
                services: vec![compiled.launch.clone()],
                readiness_timeout: compiled.readiness_timeout_ms,
            },
            registry,
            compiled.runtime_extensions(),
        )
        .await
        .map_err(|error| OpcUaError::Server(error.to_string()))?;
        session
            .start(fallback_readiness_timeout)
            .await
            .map_err(|error| OpcUaError::Server(error.to_string()))?;
        Ok(session)
    }

    async fn rebuild(&mut self, compiled: CompiledOpcUaSession) -> OpcUaResult<()> {
        self.runtime_session
            .stop()
            .await
            .map_err(|error| OpcUaError::Server(error.to_string()))?;
        self.runtime_session =
            Self::start_runtime(&self.registry, &compiled, self.fallback_readiness_timeout).await?;
        self.security_manager = Self::build_security_manager(&compiled)?;
        self.compiled = compiled;
        Ok(())
    }

    fn durable_store(&self) -> Option<DurableSubscriptionStore> {
        DurableSubscriptionStore::new(self.compiled.runtime.durability.clone())
    }

    fn durable_state_file(&self) -> Option<PathBuf> {
        if self.compiled.runtime.durability.mode != SubscriptionDurabilityMode::Persisted {
            return None;
        }
        let state_dir = self
            .compiled
            .runtime
            .durability
            .state_dir
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("mabi-opcua-subscriptions"));
        Some(state_dir.join("subscriptions.json"))
    }

    fn durable_status(&self) -> DurableSubscriptionStatus {
        self.durable_store()
            .and_then(|store| store.load_status().ok())
            .unwrap_or_else(|| DurableSubscriptionStatus {
                persisted_state_present: self
                    .durable_state_file()
                    .is_some_and(|path| path.exists()),
                restored_subscription_count: 0,
                detached_subscription_count: 0,
                last_flush_at: None,
                last_flush_result: "never_flushed".to_string(),
            })
    }

    fn diagnostics_summary(&self, durability: &DurableSubscriptionStatus) -> String {
        format!(
            "namespaces={} profile={} durability={:?} restored={} detached={}",
            self.compiled.catalog.namespace_table.len(),
            self.compiled.security.name,
            self.compiled.runtime.durability.mode,
            durability.restored_subscription_count,
            durability.detached_subscription_count,
        )
    }

    fn ensure_trust_reload_allowed(&self) -> OpcUaResult<()> {
        if self.compiled.security.allow_trust_reload {
            Ok(())
        } else {
            Err(OpcUaError::Config(format!(
                "security profile '{}' does not allow trust reload operations",
                self.compiled.security.name
            )))
        }
    }

    fn ensure_certificate_rotation_allowed(&self) -> OpcUaResult<()> {
        if self.compiled.security.allow_certificate_rotation {
            Ok(())
        } else {
            Err(OpcUaError::Config(format!(
                "security profile '{}' does not allow certificate rotation operations",
                self.compiled.security.name
            )))
        }
    }

    fn security_control_status(&self) -> SecurityControlStatus {
        SecurityControlStatus {
            profile_name: self.compiled.security.name.clone(),
            allow_trust_reload: self.compiled.security.allow_trust_reload,
            allow_certificate_rotation: self.compiled.security.allow_certificate_rotation,
            status: self.security_manager.security_status(),
        }
    }

    fn resolve_device_id(&self, target: &NodeTarget) -> OpcUaResult<String> {
        if let Some(device_id) = &target.device_id {
            return Ok(device_id.clone());
        }
        self.runtime_session
            .devices()
            .device_ids()
            .into_iter()
            .next()
            .ok_or_else(|| OpcUaError::Server("session has no registered OPC UA devices".into()))
    }

    fn resolve_point_id(&self, target: &NodeTarget) -> OpcUaResult<String> {
        if let Some(point_id) = &target.point_id {
            return Ok(point_id.clone());
        }
        if let Some(node_id) = &target.node_id {
            return Ok(node_id.clone());
        }
        Err(OpcUaError::Config(
            "node selection requires either --point or --node-id".into(),
        ))
    }

    pub async fn stop(&self) -> OpcUaResult<()> {
        self.runtime_session
            .stop()
            .await
            .map_err(|error| OpcUaError::Server(error.to_string()))
    }
}

#[async_trait]
impl SessionControlPort for OpcUaControlSession {
    async fn status(&self) -> OpcUaResult<SessionStatus> {
        let durability = self.durable_status();
        let audit_status = self.security_manager.audit_status();
        let diagnostics_summary = self.diagnostics_summary(&durability);
        Ok(SessionStatus {
            session_name: self.compiled.session_name.clone(),
            services: self.runtime_session.handles().len(),
            devices: self.runtime_session.devices().len(),
            nodes: self.compiled.catalog.nodes.len(),
            namespaces: self.compiled.catalog.namespace_table.len(),
            allow_raw_node_access: self.compiled.control.allow_raw_node_access,
            durability_mode: format!("{:?}", self.compiled.runtime.durability.mode),
            restore_on_start: self.compiled.runtime.durability.restore_on_start,
            persisted_state_present: durability.persisted_state_present,
            restored_subscriptions: durability.restored_subscription_count,
            detached_restored_subscriptions: durability.detached_subscription_count,
            last_durable_flush_at: durability.last_flush_at.map(|value| value.to_rfc3339()),
            last_durable_flush_result: durability.last_flush_result,
            diagnostics_summary,
            security_profile: self.compiled.security.name.clone(),
            audit_sink: format!("{:?}", self.compiled.security.audit_sink.kind),
            allow_trust_reload: self.compiled.security.allow_trust_reload,
            allow_certificate_rotation: self.compiled.security.allow_certificate_rotation,
            audit_status,
            generated_type_entries: self.compiled.generated_types.entries.len(),
            generated_type_module: self.compiled.generated_types.module_name.clone(),
        })
    }

    async fn snapshot(&self) -> OpcUaResult<SessionSnapshot> {
        Ok(SessionSnapshot {
            status: self.status().await?,
            services: self
                .runtime_session
                .snapshots()
                .await
                .map_err(|error| OpcUaError::Server(error.to_string()))?,
        })
    }

    async fn reset(&mut self) -> OpcUaResult<SessionSnapshot> {
        if self.compiled.control.clear_persisted_subscriptions_on_reset {
            if let Some(state_file) = self.durable_state_file() {
                match fs::remove_file(&state_file) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(OpcUaError::Server(format!(
                            "failed to clear persisted subscription state '{}': {}",
                            state_file.display(),
                            error
                        )));
                    }
                }
            }
        }
        let compiled = self.compiled.clone();
        self.rebuild(compiled).await?;
        self.snapshot().await
    }
}

impl NodeCatalogPort for OpcUaControlSession {
    fn list_nodes(&self) -> OpcUaResult<Vec<NodeDescriptor>> {
        let mut nodes = Vec::new();
        for (device_id, port) in self.runtime_session.devices().entries() {
            for point in port.point_definitions() {
                let node_id = match point.address.as_ref() {
                    Some(Address::OpcUa { node_id }) => node_id.clone(),
                    _ => point.id.clone(),
                };
                let compiled = self
                    .compiled
                    .devices
                    .iter()
                    .find(|device| device.device_id == device_id)
                    .and_then(|device| {
                        device
                            .points
                            .iter()
                            .find(|binding| binding.point_id == point.id)
                    });
                nodes.push(NodeDescriptor {
                    device_id: device_id.clone(),
                    point_id: point.id.clone(),
                    node_id,
                    browse_name: compiled
                        .map(|binding| binding.browse_name.clone())
                        .unwrap_or_else(|| point.name.clone()),
                    display_name: compiled
                        .map(|binding| binding.display_name.clone())
                        .unwrap_or_else(|| point.name.clone()),
                    node_class: compiled
                        .map(|binding| binding.node_class.clone())
                        .unwrap_or_else(|| "variable".into()),
                    writable: compiled.map(|binding| binding.writable).unwrap_or(false),
                    historizing: compiled.map(|binding| binding.historizing).unwrap_or(false),
                    sampling_interval_ms: compiled.and_then(|binding| binding.sampling_interval_ms),
                });
            }
        }
        nodes.sort_by(|left, right| {
            left.device_id
                .cmp(&right.device_id)
                .then(left.point_id.cmp(&right.point_id))
        });
        Ok(nodes)
    }
}

#[async_trait]
impl SecurityControlPort for OpcUaControlSession {
    async fn security_status(&self) -> OpcUaResult<SecurityControlStatus> {
        Ok(self.security_control_status())
    }

    async fn trust_reload(&self) -> OpcUaResult<SecurityControlStatus> {
        self.ensure_trust_reload_allowed()?;
        self.security_manager
            .reload_trust_store()
            .map_err(|error| OpcUaError::Server(error.to_string()))?;
        Ok(self.security_control_status())
    }

    async fn rotate_server_certificate(
        &self,
        certificate_path: PathBuf,
        private_key_path: PathBuf,
    ) -> OpcUaResult<SecurityControlStatus> {
        self.ensure_certificate_rotation_allowed()?;
        self.security_manager
            .rotate_server_certificate(&certificate_path, &private_key_path)
            .map_err(|error| OpcUaError::Server(error.to_string()))?;
        Ok(self.security_control_status())
    }

    async fn audit_summary(&self) -> OpcUaResult<SecurityAuditStatus> {
        Ok(self.security_manager.audit_status())
    }
}

#[async_trait]
impl NodeValueControlPort for OpcUaControlSession {
    async fn read(&self, target: &NodeTarget) -> OpcUaResult<DataPoint> {
        let device_id = self.resolve_device_id(target)?;
        let point_id = self.resolve_point_id(target)?;
        let port = self
            .runtime_session
            .devices()
            .get(&device_id)
            .ok_or_else(|| OpcUaError::NodeNotFound { node_id: device_id })?;
        port.read(&point_id).await.map_err(OpcUaError::from)
    }

    async fn write(&self, target: &NodeTarget, value: Value) -> OpcUaResult<()> {
        let device_id = self.resolve_device_id(target)?;
        let point_id = self.resolve_point_id(target)?;
        let port = self
            .runtime_session
            .devices()
            .get(&device_id)
            .ok_or_else(|| OpcUaError::NodeNotFound { node_id: device_id })?;
        port.write(&point_id, value).await.map_err(OpcUaError::from)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::modeling::{
        PresetDefinition, SessionDefinition, SimulatorDefaults, TransportDefinition,
    };

    fn compiled_session() -> CompiledOpcUaSession {
        let config = crate::modeling::OpcUaSimulatorConfig {
            defaults: SimulatorDefaults::default(),
            transports: BTreeMap::from([(
                "main".into(),
                TransportDefinition {
                    port: 0,
                    ..TransportDefinition::default()
                },
            )]),
            presets: BTreeMap::from([("generated".into(), PresetDefinition::default())]),
            sessions: BTreeMap::from([(
                "demo".into(),
                SessionDefinition {
                    transport: "main".into(),
                    preset: Some("generated".into()),
                    service_name: Some("opcua-control".into()),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };
        config.compile_session("demo", None).unwrap()
    }

    fn registry() -> ProtocolDriverRegistry {
        let mut registry = ProtocolDriverRegistry::new();
        registry.register(crate::runtime::driver());
        registry
    }

    #[tokio::test]
    async fn control_session_lists_nodes() {
        let session =
            OpcUaControlSession::new(registry(), compiled_session(), Duration::from_secs(1))
                .await
                .unwrap();
        let nodes = session.list_nodes().unwrap();
        assert!(!nodes.is_empty());
    }
}
