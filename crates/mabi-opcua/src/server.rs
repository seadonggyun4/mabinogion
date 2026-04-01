//! OPC UA server implementation.
//!
//! This module provides a high-level OPC UA server abstraction that acts as an
//! orchestration facade over the crate-private runtime builder.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    OpcUaServer                               │
//! ├─────────────────────────────────────────────────────────────┤
//! │           Lifecycle + Metrics + Public Convenience API       │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{info, instrument};

use crate::config::OpcUaServerConfig;
use crate::error::{OpcUaError, OpcUaResult};
use crate::modeling::GeneratedNodeCatalog;
#[cfg(feature = "experimental-namespace-api")]
use crate::namespace::NamespaceManagerPlugin;
use crate::nodes::{AddressSpace, NodeCache};
use crate::sdk::history::HistoryStore;
use crate::sdk::session::SessionManager;
use crate::sdk::subscription::SubscriptionManager;
use crate::server_runtime::{
    ServerBuildSpec, ServerRuntimeBuilder, ServerRuntimeHandle, ServerStatsInputs,
};
use crate::types::{DataValue, NodeId, Variant};

/// Server state enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    /// Server is not yet initialized.
    Uninitialized,
    /// Server is starting up.
    Starting,
    /// Server is running and accepting connections.
    Running,
    /// Server is shutting down.
    Stopping,
    /// Server is stopped.
    Stopped,
    /// Server encountered an error.
    Error,
}

impl Default for ServerState {
    fn default() -> Self {
        Self::Uninitialized
    }
}

/// Server statistics.
#[derive(Debug, Clone, Default)]
pub struct ServerStats {
    /// Total requests processed.
    pub total_requests: u64,
    /// Total successful requests.
    pub successful_requests: u64,
    /// Total failed requests.
    pub failed_requests: u64,
    /// Current active sessions.
    pub active_sessions: usize,
    /// Current active subscriptions.
    pub active_subscriptions: usize,
    /// Total nodes in address space.
    pub total_nodes: usize,
    /// Cache hit rate.
    pub cache_hit_rate: f64,
    /// Server uptime in seconds.
    pub uptime_seconds: u64,
}

/// Server event for external observers.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// Server started.
    Started { endpoint: String },
    /// Server stopped.
    Stopped,
    /// Client connected.
    ClientConnected { session_id: NodeId },
    /// Client disconnected.
    ClientDisconnected { session_id: NodeId },
    /// Value changed.
    ValueChanged { node_id: NodeId, value: DataValue },
    /// Error occurred.
    Error { message: String },
}

/// OPC UA Server.
///
/// The main entry point for running an OPC UA server simulation.
///
/// # Examples
///
/// ```rust,no_run
/// use mabi_opcua::{OpcUaServer, OpcUaServerConfig};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let config = OpcUaServerConfig::default();
///     let server = OpcUaServer::new(config)?;
///
///     // Add some nodes
///     server.add_variable(
///         "ns=2;i=1001",
///         "Temperature",
///         25.5f64,
///     )?;
///
///     // Start the server
///     server.start().await?;
///
///     Ok(())
/// }
/// ```
pub struct OpcUaServer {
    /// Server configuration.
    config: OpcUaServerConfig,
    /// Current server state.
    state: RwLock<ServerState>,
    /// Runtime handle that owns the assembled server subsystems.
    runtime: ServerRuntimeHandle,
    /// Server event broadcaster.
    event_tx: broadcast::Sender<ServerEvent>,
    /// Shutdown signal.
    shutdown: Arc<AtomicBool>,
    /// Start time.
    start_time: RwLock<Option<std::time::Instant>>,
    /// Request counter.
    request_counter: AtomicU64,
    /// Success counter.
    success_counter: AtomicU64,
    /// Failure counter.
    failure_counter: AtomicU64,
}

impl OpcUaServer {
    pub(crate) fn from_build_spec(spec: ServerBuildSpec) -> OpcUaResult<Self> {
        let config = spec.server_config.clone();
        let runtime = ServerRuntimeBuilder::new().build(spec)?.into_handle();
        let (event_tx, _) = broadcast::channel(1000);

        Ok(Self {
            config,
            state: RwLock::new(ServerState::Uninitialized),
            runtime,
            event_tx,
            shutdown: Arc::new(AtomicBool::new(false)),
            start_time: RwLock::new(None),
            request_counter: AtomicU64::new(0),
            success_counter: AtomicU64::new(0),
            failure_counter: AtomicU64::new(0),
        })
    }

    /// Create a new OPC UA server.
    pub fn new(config: OpcUaServerConfig) -> OpcUaResult<Self> {
        Self::from_build_spec(ServerBuildSpec::from_server_config(config))
    }

    /// Creates a server and materializes a compiled generated-node catalog into it.
    pub fn from_generated_catalog(
        config: OpcUaServerConfig,
        catalog: &GeneratedNodeCatalog,
    ) -> OpcUaResult<Self> {
        Self::from_build_spec(
            ServerBuildSpec::from_server_config(config).with_generated_catalog(catalog.clone()),
        )
    }

    /// Create a server with a builder pattern.
    pub fn builder() -> OpcUaServerBuilder {
        OpcUaServerBuilder::new()
    }

    /// Get the server configuration.
    pub fn config(&self) -> &OpcUaServerConfig {
        &self.config
    }

    /// Get the current server state.
    pub fn state(&self) -> ServerState {
        *self.state.read()
    }

    /// Get the address space.
    pub fn address_space(&self) -> &Arc<AddressSpace> {
        self.runtime.address_space()
    }

    /// Materializes a compiled catalog into the server address space.
    pub fn apply_generated_catalog(&self, catalog: &GeneratedNodeCatalog) -> OpcUaResult<()> {
        self.runtime.apply_generated_catalog(catalog)
    }

    /// Get the session manager.
    pub fn session_manager(&self) -> &Arc<SessionManager> {
        self.runtime.session_manager()
    }

    /// Get the subscription manager.
    pub fn subscription_manager(&self) -> &Arc<SubscriptionManager> {
        self.runtime.subscription_manager()
    }

    /// Get the history store.
    pub fn history_store(&self) -> &Arc<HistoryStore> {
        self.runtime.history_store()
    }

    /// Get the node cache.
    pub fn node_cache(&self) -> &Arc<NodeCache> {
        self.runtime.node_cache()
    }

    /// Subscribe to server events.
    pub fn subscribe_events(&self) -> broadcast::Receiver<ServerEvent> {
        self.event_tx.subscribe()
    }

    // =========================================================================
    // Node Operations
    // =========================================================================

    /// Add a variable node with a simple API.
    pub fn add_variable(
        &self,
        node_id: impl Into<String>,
        name: impl Into<String>,
        value: impl Into<Variant>,
    ) -> OpcUaResult<NodeId> {
        let node_id_str = node_id.into();
        let node_id: NodeId =
            node_id_str
                .parse()
                .map_err(|e: crate::types::NodeIdParseError| {
                    OpcUaError::InvalidNodeId(e.to_string())
                })?;
        let name = name.into();
        let variant = value.into();

        // Get data type from variant - use Double (11) as default
        let data_type = NodeId::numeric(0, 11);

        self.runtime.address_space().add_variable(
            node_id.clone(),
            &name,
            &name,
            data_type,
            variant,
            &NodeId::objects_folder(),
        )?;

        Ok(node_id)
    }

    /// Add a writable variable node with a simple API.
    ///
    /// Same as [`add_variable`] but sets the access level to read/write,
    /// allowing OPC UA clients to write values to this node.
    pub fn add_writable_variable(
        &self,
        node_id: impl Into<String>,
        name: impl Into<String>,
        value: impl Into<Variant>,
    ) -> OpcUaResult<NodeId> {
        let node_id_str = node_id.into();
        let node_id: NodeId =
            node_id_str
                .parse()
                .map_err(|e: crate::types::NodeIdParseError| {
                    OpcUaError::InvalidNodeId(e.to_string())
                })?;
        let name = name.into();
        let variant = value.into();

        let data_type = NodeId::numeric(0, 11);

        self.runtime.address_space().add_writable_variable(
            node_id.clone(),
            &name,
            &name,
            data_type,
            variant,
            &NodeId::objects_folder(),
        )?;

        Ok(node_id)
    }

    /// Add a folder node.
    pub fn add_folder(
        &self,
        node_id: impl Into<String>,
        name: impl Into<String>,
        parent_id: Option<&NodeId>,
    ) -> OpcUaResult<NodeId> {
        let node_id_str = node_id.into();
        let node_id: NodeId =
            node_id_str
                .parse()
                .map_err(|e: crate::types::NodeIdParseError| {
                    OpcUaError::InvalidNodeId(e.to_string())
                })?;
        let name = name.into();
        let default_parent = NodeId::objects_folder();
        let parent = parent_id.unwrap_or(&default_parent);

        self.runtime
            .address_space()
            .add_folder(node_id.clone(), &name, &name, parent)?;

        Ok(node_id)
    }

    /// Read a value from a node.
    pub fn read_value(&self, node_id: &NodeId) -> DataValue {
        self.request_counter.fetch_add(1, Ordering::Relaxed);
        let value = self.runtime.address_space().read_value(node_id);

        if value.is_good() {
            self.success_counter.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failure_counter.fetch_add(1, Ordering::Relaxed);
        }

        value
    }

    /// Write a value to a node.
    pub fn write_value(&self, node_id: &NodeId, value: impl Into<Variant>) -> OpcUaResult<()> {
        self.request_counter.fetch_add(1, Ordering::Relaxed);
        let variant = value.into();
        let data_value = DataValue::new(variant.clone());

        let status = self
            .runtime
            .address_space()
            .write_value(node_id, variant.clone());

        if status.is_good() {
            self.success_counter.fetch_add(1, Ordering::Relaxed);

            // Record in history
            self.runtime
                .history_store()
                .record_value(node_id, data_value.clone());

            // Notify subscriptions
            let subscription_manager = self.runtime.subscription_manager().clone();
            let node_id_clone = node_id.clone();
            tokio::spawn(async move {
                subscription_manager
                    .on_value_change(&node_id_clone, data_value)
                    .await;
            });

            // Broadcast event
            let _ = self.event_tx.send(ServerEvent::ValueChanged {
                node_id: node_id.clone(),
                value: DataValue::new(variant),
            });

            Ok(())
        } else {
            self.failure_counter.fetch_add(1, Ordering::Relaxed);
            Err(OpcUaError::WriteError(format!(
                "Failed to write to node {}: {:?}",
                node_id, status
            )))
        }
    }

    // =========================================================================
    // Server Lifecycle
    // =========================================================================

    /// Start the server — initializes background tasks and starts the TCP listener.
    #[instrument(skip(self))]
    pub async fn start(&self) -> OpcUaResult<()> {
        // Check current state
        {
            let mut state = self.state.write();
            if *state != ServerState::Uninitialized && *state != ServerState::Stopped {
                return Err(OpcUaError::InvalidState(format!(
                    "Cannot start server in state {:?}",
                    *state
                )));
            }
            *state = ServerState::Starting;
        }

        info!(endpoint = %self.config.endpoint_url, "Starting OPC UA server");
        self.shutdown.store(false, Ordering::Relaxed);

        // Record start time
        *self.start_time.write() = Some(std::time::Instant::now());

        self.runtime.start_transport(self.shutdown.clone()).await?;
        self.runtime.spawn_background_tasks(self.shutdown.clone());
        self.runtime.refresh_diagnostics();

        // Update state
        *self.state.write() = ServerState::Running;

        // Broadcast start event
        let _ = self.event_tx.send(ServerEvent::Started {
            endpoint: self.config.endpoint_url.clone(),
        });

        info!(endpoint = %self.config.endpoint_url, "OPC UA server started");

        Ok(())
    }

    /// Run the server (blocking) — starts the server and waits for shutdown.
    #[instrument(skip(self))]
    pub async fn run(&self) -> OpcUaResult<()> {
        self.start().await?;

        // Wait for shutdown signal
        while !self.shutdown.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        self.stop().await
    }

    /// Stop the server.
    #[instrument(skip(self))]
    pub async fn stop(&self) -> OpcUaResult<()> {
        {
            let mut state = self.state.write();
            if *state != ServerState::Running {
                return Ok(()); // Already stopped or stopping
            }
            *state = ServerState::Stopping;
        }

        info!("Stopping OPC UA server");

        // Signal shutdown
        self.shutdown.store(true, Ordering::Relaxed);
        self.runtime.close_active_sessions();
        self.runtime.stop_transport().await;
        self.runtime.shutdown_background_tasks().await;
        self.runtime.refresh_diagnostics();

        // Update state
        *self.state.write() = ServerState::Stopped;

        // Broadcast stop event
        let _ = self.event_tx.send(ServerEvent::Stopped);

        info!("OPC UA server stopped");

        Ok(())
    }

    /// Request graceful shutdown.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    /// Check if server is running.
    pub fn is_running(&self) -> bool {
        *self.state.read() == ServerState::Running
    }

    // =========================================================================
    // Statistics
    // =========================================================================

    /// Get server statistics.
    pub fn stats(&self) -> ServerStats {
        let uptime = self
            .start_time
            .read()
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);

        let ServerStatsInputs {
            active_sessions,
            active_subscriptions,
            total_nodes,
            namespace_count: _,
            security_profile_summary: _,
            durable_restore_summary: _,
            manager_ownership_summary: _,
            cache_hit_rate,
        } = self.runtime.stats_snapshot_inputs();

        ServerStats {
            total_requests: self.request_counter.load(Ordering::Relaxed),
            successful_requests: self.success_counter.load(Ordering::Relaxed),
            failed_requests: self.failure_counter.load(Ordering::Relaxed),
            active_sessions,
            active_subscriptions,
            total_nodes,
            cache_hit_rate,
            uptime_seconds: uptime,
        }
    }
}

/// Builder for OPC UA server.
pub struct OpcUaServerBuilder {
    config: OpcUaServerConfig,
    #[cfg(feature = "experimental-namespace-api")]
    namespace_managers: Vec<Arc<dyn NamespaceManagerPlugin>>,
}

impl OpcUaServerBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            config: OpcUaServerConfig::default(),
            #[cfg(feature = "experimental-namespace-api")]
            namespace_managers: Vec::new(),
        }
    }

    /// Set the endpoint URL.
    pub fn endpoint_url(mut self, url: impl Into<String>) -> Self {
        self.config.endpoint_url = url.into();
        self
    }

    /// Set the server name.
    pub fn server_name(mut self, name: impl Into<String>) -> Self {
        self.config.server_name = name.into();
        self
    }

    /// Set maximum subscriptions.
    pub fn max_subscriptions(mut self, max: usize) -> Self {
        self.config.max_subscriptions = max;
        self
    }

    /// Set maximum monitored items per subscription.
    pub fn max_monitored_items(mut self, max: usize) -> Self {
        self.config.max_monitored_items = max;
        self
    }

    /// Set minimum publishing interval.
    pub fn min_publishing_interval_ms(mut self, ms: u32) -> Self {
        self.config.min_publishing_interval_ms = ms;
        self
    }

    /// Register an experimental namespace manager plugin.
    #[cfg(feature = "experimental-namespace-api")]
    pub fn with_namespace_manager(
        mut self,
        manager: impl NamespaceManagerPlugin + 'static,
    ) -> Self {
        self.namespace_managers.push(Arc::new(manager));
        self
    }

    /// Build the server.
    pub fn build(self) -> OpcUaResult<OpcUaServer> {
        let spec = {
            let spec = ServerBuildSpec::from_server_config(self.config);
            #[cfg(feature = "experimental-namespace-api")]
            let spec = spec.with_namespace_managers(self.namespace_managers);
            spec
        };
        OpcUaServer::from_build_spec(spec)
    }
}

impl Default for OpcUaServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modeling::{CompiledNodeReference, GeneratedNodeCatalog, GeneratedNodeDefinition};
    use crate::nodes::reference::ReferenceDirection;
    use crate::nodes::{LocalizedText, QualifiedName, ReferenceTypeId};

    #[test]
    fn test_server_creation() {
        let config = OpcUaServerConfig::default();
        let server = OpcUaServer::new(config).unwrap();

        assert_eq!(server.state(), ServerState::Uninitialized);
        assert!(server.address_space().node_count() > 0); // Standard nodes
    }

    #[test]
    fn test_server_builder() {
        let server = OpcUaServer::builder()
            .endpoint_url("opc.tcp://localhost:4840")
            .server_name("Test Server")
            .max_subscriptions(100)
            .build()
            .unwrap();

        assert_eq!(server.config().endpoint_url, "opc.tcp://localhost:4840");
        assert_eq!(server.config().server_name, "Test Server");
    }

    #[test]
    fn test_add_variable() {
        let server = OpcUaServer::new(OpcUaServerConfig::default()).unwrap();

        let node_id = server
            .add_variable("ns=2;i=1001", "Temperature", 25.5f64)
            .unwrap();

        assert_eq!(node_id, NodeId::numeric(2, 1001));

        let value = server.read_value(&node_id);
        assert!(value.is_good());
        assert_eq!(value.value().unwrap().as_f64(), Some(25.5));
    }

    #[test]
    fn test_add_folder() {
        let server = OpcUaServer::new(OpcUaServerConfig::default()).unwrap();

        let folder_id = server.add_folder("ns=2;i=1000", "MyFolder", None).unwrap();

        assert!(server.address_space().contains_node(&folder_id));
    }

    #[test]
    fn test_from_generated_catalog_materializes_during_build() {
        let generated_folder = NodeId::numeric(1, 9001);
        let mut catalog = GeneratedNodeCatalog {
            namespace_table: vec![
                "http://opcfoundation.org/UA/".to_string(),
                "urn:mabinogion:test".to_string(),
            ],
            ..Default::default()
        };
        catalog.nodes.push(GeneratedNodeDefinition::Object {
            node_id: generated_folder.clone(),
            browse_name: QualifiedName::new(1, "GeneratedFolder"),
            display_name: LocalizedText::invariant("GeneratedFolder"),
            description: None,
            event_notifier: 0,
            folder_like: true,
        });
        catalog.references.push(CompiledNodeReference {
            source_node_id: NodeId::objects_folder(),
            reference_type: ReferenceTypeId::Organizes,
            target_node_id: generated_folder.clone(),
            direction: ReferenceDirection::Forward,
        });

        let server =
            OpcUaServer::from_generated_catalog(OpcUaServerConfig::default(), &catalog).unwrap();

        assert!(server.address_space().contains_node(&generated_folder));
        assert!(server
            .address_space()
            .manager_ownership_summary()
            .iter()
            .any(|summary| summary.contains("manager=catalog")));
    }

    #[tokio::test]
    async fn test_write_value() {
        let server = OpcUaServer::new(OpcUaServerConfig::default()).unwrap();

        // Add a writable variable
        server
            .address_space()
            .add_writable_variable(
                NodeId::numeric(2, 1001),
                "Temperature",
                "Temperature",
                NodeId::numeric(0, 11), // Double
                25.5f64,
                &NodeId::objects_folder(),
            )
            .unwrap();

        // Write a new value
        server
            .write_value(&NodeId::numeric(2, 1001), 30.0f64)
            .unwrap();

        // Verify
        let value = server.read_value(&NodeId::numeric(2, 1001));
        assert_eq!(value.value().unwrap().as_f64(), Some(30.0));
    }

    #[test]
    fn test_server_stats() {
        let server = OpcUaServer::new(OpcUaServerConfig::default()).unwrap();

        // Add and read a variable
        let node_id = server.add_variable("ns=2;i=1001", "Temp", 25.5f64).unwrap();
        server.read_value(&node_id);
        server.read_value(&node_id);

        let stats = server.stats();
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.successful_requests, 2);
    }

    #[tokio::test]
    async fn test_server_start_stop() {
        let config = OpcUaServerConfig {
            endpoint_url: "opc.tcp://127.0.0.1:0".to_string(),
            ..Default::default()
        };
        let server = OpcUaServer::new(config).unwrap();

        server.start().await.unwrap();
        assert_eq!(server.state(), ServerState::Running);
        assert!(server.is_running());

        server.stop().await.unwrap();
        assert_eq!(server.state(), ServerState::Stopped);
        assert!(!server.is_running());

        server.start().await.unwrap();
        assert_eq!(server.state(), ServerState::Running);
        server.stop().await.unwrap();
        assert_eq!(server.state(), ServerState::Stopped);
    }

    #[tokio::test]
    async fn test_server_refreshes_live_diagnostics() {
        let config = OpcUaServerConfig {
            endpoint_url: "opc.tcp://127.0.0.1:0".to_string(),
            ..Default::default()
        };
        let server = OpcUaServer::new(config).unwrap();

        server.start().await.unwrap();

        let ns = server
            .address_space()
            .get_namespace_index(crate::sdk::address_space::DiagnosticsNodeManager::NAMESPACE_URI)
            .unwrap();
        let session_count = server
            .address_space()
            .read_value(&NodeId::string(ns, "Diagnostics.CurrentSessionCount"));
        let namespace_count = server
            .address_space()
            .read_value(&NodeId::string(ns, "Diagnostics.NamespaceCount"));
        let security_summary = server
            .address_space()
            .read_value(&NodeId::string(ns, "Diagnostics.SecurityProfileSummary"));

        assert_eq!(session_count.value().and_then(|v| v.as_u32()), Some(0));
        assert!(
            namespace_count
                .value()
                .and_then(|v| v.as_u32())
                .unwrap_or(0)
                >= 2
        );
        assert!(security_summary
            .value()
            .and_then(|v| v.as_str())
            .is_some_and(|summary| !summary.is_empty()));

        server.stop().await.unwrap();
    }
}
