//! In-process control-plane surface for compiled OPC UA simulator sessions.

use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;

use mabi_core::types::{Address, DataPoint};
use mabi_core::value::Value;
use mabi_runtime::{ProtocolDriverRegistry, RuntimeSession, RuntimeSessionSpec};

use crate::error::{OpcUaError, OpcUaResult};
use crate::modeling::CompiledOpcUaSession;

/// Lifecycle-oriented control surface for a compiled session.
#[async_trait]
pub trait SessionControlPort: Send {
    async fn status(&self) -> OpcUaResult<SessionStatus>;
    async fn snapshot(&self) -> OpcUaResult<SessionSnapshot>;
    async fn reset(&mut self) -> OpcUaResult<SessionSnapshot>;
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
}

/// Snapshot returned by reset and snapshot operations.
#[derive(Debug, Clone, Serialize)]
pub struct SessionSnapshot {
    pub status: SessionStatus,
    pub services: Vec<mabi_runtime::ServiceSnapshot>,
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
}

impl OpcUaControlSession {
    pub async fn new(
        registry: ProtocolDriverRegistry,
        compiled: CompiledOpcUaSession,
        fallback_readiness_timeout: Duration,
    ) -> OpcUaResult<Self> {
        let runtime_session =
            Self::start_runtime(&registry, &compiled, fallback_readiness_timeout).await?;
        Ok(Self {
            registry,
            compiled,
            fallback_readiness_timeout,
            runtime_session,
        })
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
        self.compiled = compiled;
        Ok(())
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
        Ok(SessionStatus {
            session_name: self.compiled.session_name.clone(),
            services: self.runtime_session.handles().len(),
            devices: self.runtime_session.devices().len(),
            nodes: self.compiled.catalog.nodes.len(),
            namespaces: self.compiled.catalog.namespace_table.len(),
            allow_raw_node_access: self.compiled.control.allow_raw_node_access,
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
