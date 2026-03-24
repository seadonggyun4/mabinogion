//! OPC UA server implementation.
//!
//! This module provides a high-level OPC UA server abstraction that integrates:
//! - Session management
//! - Subscription management
//! - Address space with node operations
//! - Historical data access
//! - Node caching and prefetching
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    OpcUaServer                               │
//! ├─────────────────────────────────────────────────────────────┤
//! │  ┌─────────────┐  ┌──────────────┐  ┌───────────────┐       │
//! │  │   Session   │  │ Subscription │  │    History    │       │
//! │  │   Manager   │  │   Manager    │  │    Store      │       │
//! │  └─────────────┘  └──────────────┘  └───────────────┘       │
//! ├─────────────────────────────────────────────────────────────┤
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │              Address Space (Nodes + Cache)               ││
//! │  └─────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────┘
//! ```

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, info, instrument, warn};

use crate::config::OpcUaServerConfig;
use crate::error::{OpcUaError, OpcUaResult};
use crate::nodes::{
    AddressSpace, AddressSpaceConfig, NodeCache, NodeCacheConfig, NodePrefetcher, PrefetchConfig,
};
use crate::security::{SecurityManager, SecurityManagerConfig};
use crate::service::registry::ServiceRegistry;
use crate::services::{
    HistoryStore, HistoryStoreConfig, SessionManager, SessionManagerConfig, SubscriptionManager,
    SubscriptionManagerConfig,
};
use crate::transport::connection::ServiceContextTemplate;
use crate::transport::tcp_listener::{OpcUaTcpListener, TcpTransportConfig};
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
    /// Session manager.
    session_manager: Arc<SessionManager>,
    /// Subscription manager.
    subscription_manager: Arc<SubscriptionManager>,
    /// Address space.
    address_space: Arc<AddressSpace>,
    /// History store.
    history_store: Arc<HistoryStore>,
    /// Node cache.
    node_cache: Arc<NodeCache>,
    /// Node prefetcher.
    node_prefetcher: Arc<NodePrefetcher>,
    /// Security manager.
    security_manager: Arc<SecurityManager>,
    /// TCP listener instance tracked for graceful shutdown.
    tcp_listener: RwLock<Option<Arc<OpcUaTcpListener>>>,
    /// TCP listener task.
    tcp_listener_task: RwLock<Option<JoinHandle<()>>>,
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
    /// Create a new OPC UA server.
    pub fn new(config: OpcUaServerConfig) -> OpcUaResult<Self> {
        // Create session manager
        let session_config = SessionManagerConfig {
            max_sessions: 1000,
            session_timeout_ms: 60_000,
            max_subscriptions_per_session: config.max_subscriptions,
        };
        let session_manager = Arc::new(SessionManager::with_config(session_config));

        // Create subscription manager
        let subscription_config = SubscriptionManagerConfig {
            max_subscriptions: config.max_subscriptions,
            max_monitored_items_per_subscription: config.max_monitored_items,
            notification_buffer_size: 10_000,
            event_buffer_size: 1000,
        };
        let subscription_manager = Arc::new(SubscriptionManager::with_config(subscription_config));

        // Create address space
        let address_space_config = AddressSpaceConfig {
            max_nodes: 1_000_000,
            max_references_per_node: 10_000,
            ..Default::default()
        };
        let address_space = Arc::new(AddressSpace::new(address_space_config));

        // Create history store
        let history_config = HistoryStoreConfig::default();
        let history_store = Arc::new(HistoryStore::new(history_config));

        // Create node cache
        let cache_config = NodeCacheConfig {
            max_size: 100_000,
            prefetch_enabled: true,
            prefetch_depth: 2,
            cache_values: true,
            value_cache_ttl_ms: 1000,
        };
        let node_cache = Arc::new(NodeCache::new(cache_config));

        // Create prefetcher
        let prefetch_config = PrefetchConfig::default();
        let node_prefetcher = Arc::new(NodePrefetcher::new(
            prefetch_config,
            node_cache.clone(),
            address_space.clone(),
        ));

        // Create security manager
        let security_config = SecurityManagerConfig::default();
        let security_manager = Arc::new(SecurityManager::new(security_config));

        // Create event channel
        let (event_tx, _) = broadcast::channel(1000);

        Ok(Self {
            config,
            state: RwLock::new(ServerState::Uninitialized),
            session_manager,
            subscription_manager,
            address_space,
            history_store,
            node_cache,
            node_prefetcher,
            security_manager,
            tcp_listener: RwLock::new(None),
            tcp_listener_task: RwLock::new(None),
            event_tx,
            shutdown: Arc::new(AtomicBool::new(false)),
            start_time: RwLock::new(None),
            request_counter: AtomicU64::new(0),
            success_counter: AtomicU64::new(0),
            failure_counter: AtomicU64::new(0),
        })
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
        &self.address_space
    }

    /// Get the session manager.
    pub fn session_manager(&self) -> &Arc<SessionManager> {
        &self.session_manager
    }

    /// Get the subscription manager.
    pub fn subscription_manager(&self) -> &Arc<SubscriptionManager> {
        &self.subscription_manager
    }

    /// Get the history store.
    pub fn history_store(&self) -> &Arc<HistoryStore> {
        &self.history_store
    }

    /// Get the node cache.
    pub fn node_cache(&self) -> &Arc<NodeCache> {
        &self.node_cache
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

        self.address_space.add_variable(
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

        self.address_space.add_writable_variable(
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

        self.address_space
            .add_folder(node_id.clone(), &name, &name, parent)?;

        Ok(node_id)
    }

    /// Read a value from a node.
    pub fn read_value(&self, node_id: &NodeId) -> DataValue {
        self.request_counter.fetch_add(1, Ordering::Relaxed);
        let value = self.address_space.read_value(node_id);

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

        let status = self.address_space.write_value(node_id, variant.clone());

        if status.is_good() {
            self.success_counter.fetch_add(1, Ordering::Relaxed);

            // Record in history
            self.history_store.record_value(node_id, data_value.clone());

            // Notify subscriptions
            let subscription_manager = self.subscription_manager.clone();
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

        // Start background tasks
        self.start_background_tasks().await;

        // Start TCP listener in a background task
        let tcp_listener = Arc::new(self.create_tcp_listener()?);
        let listener_task = {
            let tcp_listener = tcp_listener.clone();
            tokio::spawn(async move {
                if let Err(e) = tcp_listener.run().await {
                    warn!(error = %e, "TCP listener error");
                }
            })
        };
        *self.tcp_listener.write() = Some(tcp_listener);
        *self.tcp_listener_task.write() = Some(listener_task);

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

    /// Create the TCP listener with service registry and context.
    fn create_tcp_listener(&self) -> OpcUaResult<OpcUaTcpListener> {
        // Parse bind address from endpoint URL
        let bind_address = parse_endpoint_url(&self.config.endpoint_url)?;

        // Build service registry with all handlers
        let mut registry = ServiceRegistry::new();
        crate::service::discovery::register_handlers(&mut registry);
        crate::service::session::register_handlers(&mut registry);
        crate::service::attribute::register_handlers(&mut registry);
        crate::service::browse::register_handlers(&mut registry);
        crate::service::subscription::register_handlers(&mut registry);
        crate::service::monitored_item::register_handlers(&mut registry);
        crate::service::register_nodes::register_handlers(&mut registry);
        crate::service::translate_browse_paths::register_handlers(&mut registry);
        crate::service::transfer_subscription::register_handlers(&mut registry);
        crate::service::history::register_handlers(&mut registry);
        crate::service::method_call::register_handlers(&mut registry);

        info!(
            handlers = registry.handler_count(),
            "Registered service handlers"
        );

        // Create method registry with sample methods
        let method_registry = Arc::new(crate::service::method_call::MethodRegistry::new());
        self.register_default_methods(&method_registry);

        // Build shared context template
        let context = Arc::new(ServiceContextTemplate {
            session_manager: self.session_manager.clone(),
            address_space: self.address_space.clone(),
            subscription_manager: self.subscription_manager.clone(),
            history_store: self.history_store.clone(),
            security_manager: self.security_manager.clone(),
            server_config: Arc::new(self.config.clone()),
            method_registry,
        });

        let tcp_config = TcpTransportConfig {
            bind_address,
            max_connections: 1000,
            connection_timeout: Duration::from_secs(60),
            server_buffer_size: 65535,
        };

        Ok(OpcUaTcpListener::new(
            tcp_config,
            Arc::new(registry),
            context,
        ))
    }

    /// Register default server methods (sample callable methods for testing).
    ///
    /// Creates a Multiply method node with InputArguments/OutputArguments
    /// property nodes so TRAP's MethodCallManager can discover and call it.
    fn register_default_methods(
        &self,
        method_registry: &Arc<crate::service::method_call::MethodRegistry>,
    ) {
        use crate::nodes::classes::MethodNode;
        use crate::nodes::reference::Reference;

        // Multiply method (ns=0; i=62541) — sample method for testing
        let method_node_id = NodeId::numeric(0, 62541);

        // Create method node in address space
        let method_node = MethodNode::new(method_node_id.clone(), "Multiply", "Multiply");
        self.address_space.insert_node(method_node);
        self.address_space.add_reference(Reference::has_component(
            NodeId::server(), // Parent: Server object
            method_node_id.clone(),
        ));

        // InputArguments property (ns=0; i=62542)
        let input_args_id = NodeId::numeric(0, 62542);
        let input_args_var = crate::nodes::classes::VariableNode::new(
            input_args_id.clone(),
            "InputArguments",
            "InputArguments",
            NodeId::numeric(0, 296), // Argument DataType
            Variant::Null,           // Placeholder — TRAP reads the structure via attribute
        );
        self.address_space.insert_node(input_args_var);
        self.address_space.add_reference(Reference::has_property(
            method_node_id.clone(),
            input_args_id,
        ));

        // OutputArguments property (ns=0; i=62543)
        let output_args_id = NodeId::numeric(0, 62543);
        let output_args_var = crate::nodes::classes::VariableNode::new(
            output_args_id.clone(),
            "OutputArguments",
            "OutputArguments",
            NodeId::numeric(0, 296), // Argument DataType
            Variant::Null,
        );
        self.address_space.insert_node(output_args_var);
        self.address_space.add_reference(Reference::has_property(
            method_node_id.clone(),
            output_args_id,
        ));

        // Register the actual callback
        method_registry.register(
            method_node_id,
            Arc::new(|args: &[Variant]| {
                if args.len() < 2 {
                    return Err(crate::types::StatusCode::BAD_INVALID_ARGUMENT);
                }
                let a = args[0]
                    .as_f64()
                    .ok_or(crate::types::StatusCode::BAD_INVALID_ARGUMENT)?;
                let b = args[1]
                    .as_f64()
                    .ok_or(crate::types::StatusCode::BAD_INVALID_ARGUMENT)?;
                Ok(vec![Variant::Double(a * b)])
            }),
        );
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
        if let Some(listener) = self.tcp_listener.read().as_ref() {
            listener.shutdown();
        }

        // Close all sessions
        for session_id in self.session_manager.session_ids() {
            let _ = self.session_manager.close_session(&session_id);
        }

        let listener_task = self.tcp_listener_task.write().take();
        if let Some(listener_task) = listener_task {
            let _ = tokio::time::timeout(Duration::from_secs(5), listener_task).await;
        }
        self.tcp_listener.write().take();

        // Wait for background tasks to observe cancellation
        tokio::time::sleep(Duration::from_millis(100)).await;

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

        let cache_stats = self.node_cache.stats();

        ServerStats {
            total_requests: self.request_counter.load(Ordering::Relaxed),
            successful_requests: self.success_counter.load(Ordering::Relaxed),
            failed_requests: self.failure_counter.load(Ordering::Relaxed),
            active_sessions: self.session_manager.session_count(),
            active_subscriptions: self.subscription_manager.subscription_count(),
            total_nodes: self.address_space.node_count(),
            cache_hit_rate: cache_stats.hit_rate(),
            uptime_seconds: uptime,
        }
    }

    // =========================================================================
    // Private Methods
    // =========================================================================

    async fn start_background_tasks(&self) {
        // Subscription processing task
        let subscription_manager = self.subscription_manager.clone();
        let shutdown = self.shutdown.clone();
        tokio::spawn(async move {
            let interval = Duration::from_millis(100);
            while !shutdown.load(Ordering::Relaxed) {
                subscription_manager.process_all().await;
                tokio::time::sleep(interval).await;
            }
        });

        // Session cleanup task
        let session_manager = self.session_manager.clone();
        let shutdown = self.shutdown.clone();
        tokio::spawn(async move {
            let interval = Duration::from_secs(10);
            while !shutdown.load(Ordering::Relaxed) {
                session_manager.cleanup_expired();
                tokio::time::sleep(interval).await;
            }
        });

        // History cleanup task
        let history_store = self.history_store.clone();
        let shutdown = self.shutdown.clone();
        tokio::spawn(async move {
            let interval = Duration::from_secs(3600); // 1 hour
            while !shutdown.load(Ordering::Relaxed) {
                history_store.cleanup();
                tokio::time::sleep(interval).await;
            }
        });

        // Prefetch processing task
        let prefetcher = self.node_prefetcher.clone();
        let shutdown = self.shutdown.clone();
        tokio::spawn(async move {
            let interval = Duration::from_millis(10);
            while !shutdown.load(Ordering::Relaxed) {
                prefetcher.process_pending();
                tokio::time::sleep(interval).await;
            }
        });

        debug!("Background tasks started");
    }
}

/// Parse an OPC UA endpoint URL (e.g., "opc.tcp://0.0.0.0:4840/") into a SocketAddr.
fn parse_endpoint_url(url: &str) -> OpcUaResult<SocketAddr> {
    // Strip protocol prefix
    let addr_part = url.strip_prefix("opc.tcp://").unwrap_or(url);
    // Strip trailing path
    let addr_part = addr_part.split('/').next().unwrap_or(addr_part);

    addr_part.parse::<SocketAddr>().map_err(|e| {
        OpcUaError::Server(format!(
            "Failed to parse endpoint URL '{}' as socket address: {}",
            url, e
        ))
    })
}

/// Builder for OPC UA server.
pub struct OpcUaServerBuilder {
    config: OpcUaServerConfig,
}

impl OpcUaServerBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            config: OpcUaServerConfig::default(),
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

    /// Build the server.
    pub fn build(self) -> OpcUaResult<OpcUaServer> {
        OpcUaServer::new(self.config)
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
        let server = OpcUaServer::new(OpcUaServerConfig::default()).unwrap();

        server.start().await.unwrap();
        assert_eq!(server.state(), ServerState::Running);
        assert!(server.is_running());

        server.stop().await.unwrap();
        assert_eq!(server.state(), ServerState::Stopped);
        assert!(!server.is_running());
    }
}
