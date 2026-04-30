mod methods;

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::info;

use crate::config::{OpcUaServerConfig, TransportConnectionMode, TransportProtocol};
use crate::core::services::BuiltinServiceSet;
use crate::error::{OpcUaError, OpcUaResult};
use crate::modeling::GeneratedNodeCatalog;
#[cfg(feature = "experimental-namespace-api")]
use crate::namespace::{adapt_namespace_manager_plugin, NamespaceManagerPlugin};
use crate::nodes::{
    AddressSpace, AddressSpaceConfig, NodeCache, NodeCacheConfig, NodePrefetcher, PrefetchConfig,
};
use crate::sdk::address_space::{CatalogNodeManager, DiagnosticsSnapshot, NodeManager};
use crate::sdk::history::{HistoryStore, HistoryStoreConfig};
use crate::sdk::session::{SessionManager, SessionManagerConfig};
use crate::sdk::subscription::{SubscriptionManager, SubscriptionManagerConfig};
use crate::security::{SecurityManager, SecurityManagerConfig};
use crate::transport::adapter::{
    ConnectionInitiationMode, TransportAdapterConfig, TransportListener,
};
use crate::transport::connection::ServiceContextTemplate;
use crate::transport::hooks::TransportHooks;
use crate::transport::https_listener::{HttpsTransportConfig, OpcUaHttpsListener};
use crate::transport::runtime::{TransportRuntime, TransportRuntimePolicy};
use crate::transport::tcp_listener::{OpcUaTcpListener, TcpTransportConfig};
use crate::transport::tcp_reverse_connector::{TcpReverseConnectConfig, TcpReverseConnector};

use self::methods::build_method_registry;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MethodRegistryPreset {
    Default,
    Empty,
}

impl Default for MethodRegistryPreset {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ServerBuildDefaults {
    pub(crate) session: SessionManagerConfig,
    pub(crate) subscription: SubscriptionManagerConfig,
    pub(crate) address_space: AddressSpaceConfig,
    pub(crate) history_store: HistoryStoreConfig,
    pub(crate) node_cache: NodeCacheConfig,
    pub(crate) prefetch: PrefetchConfig,
    pub(crate) security: SecurityManagerConfig,
    pub(crate) tcp: TcpTransportConfig,
}

impl ServerBuildDefaults {
    pub(crate) fn for_server_config(config: &OpcUaServerConfig) -> Self {
        let mut security = SecurityManagerConfig::default();
        security.certificate_config.certificate_path = config.certificate_path.clone();
        security.certificate_config.private_key_path = config.private_key_path.clone();
        Self {
            session: SessionManagerConfig {
                max_sessions: 1000,
                session_timeout_ms: 60_000,
                max_subscriptions_per_session: config.max_subscriptions,
            },
            subscription: SubscriptionManagerConfig {
                max_subscriptions: config.max_subscriptions,
                max_monitored_items_per_subscription: config.max_monitored_items,
                notification_buffer_size: 10_000,
                event_buffer_size: 1000,
                durability: Default::default(),
            },
            address_space: AddressSpaceConfig {
                max_nodes: 1_000_000,
                max_references_per_node: 10_000,
                ..Default::default()
            },
            history_store: HistoryStoreConfig::default(),
            node_cache: NodeCacheConfig {
                max_size: 100_000,
                prefetch_enabled: true,
                prefetch_depth: 2,
                cache_values: true,
                value_cache_ttl_ms: 1000,
            },
            prefetch: PrefetchConfig::default(),
            security,
            tcp: TcpTransportConfig {
                bind_address: SocketAddr::from(([0, 0, 0, 0], 4840)),
                max_connections: 1000,
                connection_timeout: Duration::from_secs(60),
                server_buffer_size: 65_535,
                initiation_mode: ConnectionInitiationMode::Listener,
            },
        }
    }
}

#[derive(Clone)]
pub(crate) struct ServerBuildSpec {
    pub(crate) server_config: OpcUaServerConfig,
    pub(crate) generated_catalog: Option<GeneratedNodeCatalog>,
    pub(crate) method_registry_preset: MethodRegistryPreset,
    pub(crate) transport_policy: TransportRuntimePolicy,
    pub(crate) transport_hooks: TransportHooks,
    pub(crate) defaults: ServerBuildDefaults,
    #[cfg(feature = "experimental-namespace-api")]
    pub(crate) namespace_managers: Vec<Arc<dyn NamespaceManagerPlugin>>,
}

impl ServerBuildSpec {
    pub(crate) fn from_server_config(server_config: OpcUaServerConfig) -> Self {
        let defaults = ServerBuildDefaults::for_server_config(&server_config);
        let transport_policy = TransportRuntimePolicy {
            connection_timeout: defaults.tcp.connection_timeout,
            server_buffer_size: defaults.tcp.server_buffer_size,
        };
        Self {
            server_config,
            generated_catalog: None,
            method_registry_preset: MethodRegistryPreset::Default,
            transport_policy,
            transport_hooks: TransportHooks::new(),
            defaults,
            #[cfg(feature = "experimental-namespace-api")]
            namespace_managers: Vec::new(),
        }
    }

    pub(crate) fn with_generated_catalog(
        mut self,
        generated_catalog: GeneratedNodeCatalog,
    ) -> Self {
        self.generated_catalog = Some(generated_catalog);
        self
    }

    #[cfg(feature = "experimental-namespace-api")]
    pub(crate) fn with_namespace_managers(
        mut self,
        namespace_managers: Vec<Arc<dyn NamespaceManagerPlugin>>,
    ) -> Self {
        self.namespace_managers = namespace_managers;
        self
    }
}

pub(crate) struct ServerRuntimeBuilder;

impl ServerRuntimeBuilder {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn build(self, spec: ServerBuildSpec) -> OpcUaResult<BuiltServerRuntime> {
        let session_manager = Arc::new(SessionManager::with_config(spec.defaults.session.clone()));
        let subscription_manager = Arc::new(SubscriptionManager::with_config(
            spec.defaults.subscription.clone(),
        ));
        let mut address_space_config = spec.defaults.address_space.clone();
        if let Some(namespace_uri) = spec
            .generated_catalog
            .as_ref()
            .and_then(|catalog| catalog.namespace_table.get(1))
        {
            address_space_config.default_namespace_uri = namespace_uri.clone();
        }
        let address_space = Arc::new(AddressSpace::new_with_internal_managers(
            address_space_config,
            build_internal_managers(
                spec.generated_catalog.as_ref(),
                #[cfg(feature = "experimental-namespace-api")]
                &spec.namespace_managers,
            ),
        ));
        let history_store = Arc::new(HistoryStore::new(spec.defaults.history_store.clone()));
        let node_cache = Arc::new(NodeCache::new(spec.defaults.node_cache.clone()));
        let node_prefetcher = Arc::new(NodePrefetcher::new(
            spec.defaults.prefetch.clone(),
            node_cache.clone(),
            address_space.clone(),
        ));
        let security_manager = Arc::new(SecurityManager::new(spec.defaults.security.clone()));

        if let Some(catalog) = &spec.generated_catalog {
            catalog.materialize(&address_space)?;
        }

        let method_registry = build_method_registry(spec.method_registry_preset, &address_space);
        let context_template = Arc::new(ServiceContextTemplate {
            session_manager: session_manager.clone(),
            address_space: address_space.clone(),
            subscription_manager: subscription_manager.clone(),
            history_store: history_store.clone(),
            security_manager: security_manager.clone(),
            server_config: Arc::new(spec.server_config.clone()),
            method_registry,
        });

        let builtins = Arc::new(BuiltinServiceSet::all());
        info!(
            handlers = builtins.len(),
            "Registered built-in service handlers"
        );

        let mut transport_config = build_transport_config(&spec.server_config, &spec.defaults)?;
        match &mut transport_config {
            TransportAdapterConfig::TcpListener(config) => {
                config.connection_timeout = spec.transport_policy.connection_timeout;
                config.server_buffer_size = spec.transport_policy.server_buffer_size;
            }
            TransportAdapterConfig::TcpReverse(config) => {
                config.connection_timeout = spec.transport_policy.connection_timeout;
                config.server_buffer_size = spec.transport_policy.server_buffer_size;
            }
            TransportAdapterConfig::Https(config) => {
                config.connection_timeout = spec.transport_policy.connection_timeout;
            }
        }

        let transport_runtime = Arc::new(TransportRuntime::new(
            builtins.clone(),
            context_template,
            spec.transport_policy,
            spec.transport_hooks,
        ));

        Ok(BuiltServerRuntime {
            session_manager,
            subscription_manager,
            address_space,
            history_store,
            node_cache,
            node_prefetcher,
            security_manager,
            builtins,
            transport_config,
            transport_runtime,
        })
    }
}

pub(crate) struct BuiltServerRuntime {
    session_manager: Arc<SessionManager>,
    subscription_manager: Arc<SubscriptionManager>,
    address_space: Arc<AddressSpace>,
    history_store: Arc<HistoryStore>,
    node_cache: Arc<NodeCache>,
    node_prefetcher: Arc<NodePrefetcher>,
    security_manager: Arc<SecurityManager>,
    builtins: Arc<BuiltinServiceSet>,
    transport_config: TransportAdapterConfig,
    transport_runtime: Arc<TransportRuntime>,
}

impl BuiltServerRuntime {
    pub(crate) fn into_handle(self) -> ServerRuntimeHandle {
        ServerRuntimeHandle {
            session_manager: self.session_manager,
            subscription_manager: self.subscription_manager,
            address_space: self.address_space,
            history_store: self.history_store,
            node_cache: self.node_cache,
            node_prefetcher: self.node_prefetcher,
            security_manager: self.security_manager,
            builtins: self.builtins,
            transport_config: self.transport_config,
            transport_runtime: self.transport_runtime,
            transport_listener: RwLock::new(None),
            transport_listener_task: RwLock::new(None),
            background_tasks: RwLock::new(None),
        }
    }
}

pub(crate) struct ServerRuntimeHandle {
    session_manager: Arc<SessionManager>,
    subscription_manager: Arc<SubscriptionManager>,
    address_space: Arc<AddressSpace>,
    history_store: Arc<HistoryStore>,
    node_cache: Arc<NodeCache>,
    node_prefetcher: Arc<NodePrefetcher>,
    security_manager: Arc<SecurityManager>,
    #[allow(dead_code)]
    builtins: Arc<BuiltinServiceSet>,
    transport_config: TransportAdapterConfig,
    transport_runtime: Arc<TransportRuntime>,
    transport_listener: RwLock<Option<TransportListener>>,
    transport_listener_task: RwLock<Option<JoinHandle<()>>>,
    background_tasks: RwLock<Option<ServerBackgroundTasks>>,
}

impl ServerRuntimeHandle {
    pub(crate) fn address_space(&self) -> &Arc<AddressSpace> {
        &self.address_space
    }

    pub(crate) fn session_manager(&self) -> &Arc<SessionManager> {
        &self.session_manager
    }

    pub(crate) fn subscription_manager(&self) -> &Arc<SubscriptionManager> {
        &self.subscription_manager
    }

    pub(crate) fn history_store(&self) -> &Arc<HistoryStore> {
        &self.history_store
    }

    pub(crate) fn node_cache(&self) -> &Arc<NodeCache> {
        &self.node_cache
    }

    pub(crate) fn apply_generated_catalog(
        &self,
        catalog: &GeneratedNodeCatalog,
    ) -> OpcUaResult<()> {
        catalog.materialize(&self.address_space)
    }

    pub(crate) async fn start_transport(&self, shutdown: Arc<AtomicBool>) -> OpcUaResult<()> {
        if self.transport_listener.read().is_some() {
            return Ok(());
        }

        let listener = match &self.transport_config {
            TransportAdapterConfig::TcpListener(config) => {
                TransportListener::TcpListener(Arc::new(OpcUaTcpListener::new(
                    config.clone(),
                    self.transport_runtime.clone(),
                )))
            }
            TransportAdapterConfig::TcpReverse(config) => TransportListener::TcpReverse(Arc::new(
                TcpReverseConnector::new(config.clone(), self.transport_runtime.clone()),
            )),
            TransportAdapterConfig::Https(config) => {
                config.validate()?;
                TransportListener::Https(Arc::new(OpcUaHttpsListener::new(
                    config.clone(),
                    self.transport_runtime.clone(),
                )))
            }
        };
        let listener_task = {
            let listener = listener.clone();
            tokio::spawn(async move {
                if let Err(error) = listener.run().await {
                    tracing::warn!(error = %error, "Transport listener error");
                }
            })
        };

        shutdown.store(false, Ordering::Relaxed);
        *self.transport_listener.write() = Some(listener);
        *self.transport_listener_task.write() = Some(listener_task);
        let snapshot = self.stats_snapshot_inputs();
        self.address_space.on_runtime_start(&DiagnosticsSnapshot {
            current_sessions: snapshot.active_sessions as u32,
            current_subscriptions: snapshot.active_subscriptions as u32,
            total_nodes: snapshot.total_nodes as u32,
            namespace_count: snapshot.namespace_count as u32,
            security_profile_summary: snapshot.security_profile_summary,
            durable_restore_summary: snapshot.durable_restore_summary,
            manager_ownership_summary: snapshot.manager_ownership_summary,
        });
        Ok(())
    }

    pub(crate) async fn stop_transport(&self) {
        let snapshot = self.stats_snapshot_inputs();
        self.address_space.on_runtime_stop(&DiagnosticsSnapshot {
            current_sessions: snapshot.active_sessions as u32,
            current_subscriptions: snapshot.active_subscriptions as u32,
            total_nodes: snapshot.total_nodes as u32,
            namespace_count: snapshot.namespace_count as u32,
            security_profile_summary: snapshot.security_profile_summary,
            durable_restore_summary: snapshot.durable_restore_summary,
            manager_ownership_summary: snapshot.manager_ownership_summary,
        });

        if let Some(listener) = self.transport_listener.read().as_ref() {
            listener.shutdown();
        }

        let listener_task = self.transport_listener_task.write().take();
        if let Some(listener_task) = listener_task {
            let _ = tokio::time::timeout(Duration::from_secs(5), listener_task).await;
        }
        self.transport_listener.write().take();
    }

    pub(crate) fn spawn_background_tasks(&self, shutdown: Arc<AtomicBool>) {
        let mut tasks_guard = self.background_tasks.write();
        if tasks_guard.is_some() {
            return;
        }
        *tasks_guard = Some(ServerBackgroundTasks::spawn(
            self.subscription_manager.clone(),
            self.session_manager.clone(),
            self.history_store.clone(),
            self.node_prefetcher.clone(),
            self.address_space.clone(),
            self.security_manager.clone(),
            shutdown,
        ));
    }

    pub(crate) async fn shutdown_background_tasks(&self) {
        let tasks = self.background_tasks.write().take();
        if let Some(tasks) = tasks {
            tasks.shutdown().await;
        }
    }

    pub(crate) fn close_active_sessions(&self) {
        for session_id in self.session_manager.session_ids() {
            let _ = self.session_manager.close_session(&session_id);
        }
    }

    pub(crate) fn stats_snapshot_inputs(&self) -> ServerStatsInputs {
        let cache_stats = self.node_cache.stats();
        ServerStatsInputs {
            active_sessions: self.session_manager.session_count(),
            active_subscriptions: self.subscription_manager.subscription_count(),
            total_nodes: self.address_space.node_count(),
            namespace_count: self.address_space.namespaces().len(),
            security_profile_summary: self.security_manager.diagnostics_summary(),
            durable_restore_summary: format!(
                "{:?}/restored={}/detached={}",
                self.subscription_manager.durability_mode(),
                self.subscription_manager.restored_subscription_count(),
                self.subscription_manager.detached_subscription_count()
            ),
            manager_ownership_summary: self.address_space.manager_ownership_summary().join("; "),
            cache_hit_rate: cache_stats.hit_rate(),
        }
    }

    pub(crate) fn refresh_diagnostics(&self) {
        let snapshot = self.stats_snapshot_inputs();
        self.address_space
            .refresh_diagnostics(&DiagnosticsSnapshot {
                current_sessions: snapshot.active_sessions as u32,
                current_subscriptions: snapshot.active_subscriptions as u32,
                total_nodes: snapshot.total_nodes as u32,
                namespace_count: snapshot.namespace_count as u32,
                security_profile_summary: snapshot.security_profile_summary,
                durable_restore_summary: snapshot.durable_restore_summary,
                manager_ownership_summary: snapshot.manager_ownership_summary,
            });
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ServerStatsInputs {
    pub(crate) active_sessions: usize,
    pub(crate) active_subscriptions: usize,
    pub(crate) total_nodes: usize,
    pub(crate) namespace_count: usize,
    pub(crate) security_profile_summary: String,
    pub(crate) durable_restore_summary: String,
    pub(crate) manager_ownership_summary: String,
    pub(crate) cache_hit_rate: f64,
}

pub(crate) struct ServerBackgroundTasks {
    shutdown_tx: broadcast::Sender<()>,
    handles: Vec<JoinHandle<()>>,
}

impl ServerBackgroundTasks {
    fn spawn(
        subscription_manager: Arc<SubscriptionManager>,
        session_manager: Arc<SessionManager>,
        history_store: Arc<HistoryStore>,
        node_prefetcher: Arc<NodePrefetcher>,
        address_space: Arc<AddressSpace>,
        security_manager: Arc<SecurityManager>,
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let subscription_manager_for_tick = subscription_manager.clone();
        let subscription_manager_for_diagnostics = subscription_manager.clone();
        let session_manager_for_cleanup = session_manager.clone();
        let session_manager_for_diagnostics = session_manager.clone();
        let history_store_for_cleanup = history_store.clone();
        let node_prefetcher_for_processing = node_prefetcher.clone();
        let address_space_for_diagnostics = address_space.clone();
        let security_manager_for_diagnostics = security_manager.clone();
        let handles = vec![
            spawn_shutdown_task(
                shutdown.clone(),
                shutdown_tx.subscribe(),
                Duration::from_millis(100),
                move || {
                    let subscription_manager = subscription_manager_for_tick.clone();
                    async move { subscription_manager.process_all().await }
                },
            ),
            spawn_shutdown_task(
                shutdown.clone(),
                shutdown_tx.subscribe(),
                Duration::from_secs(10),
                move || {
                    let session_manager = session_manager_for_cleanup.clone();
                    async move { session_manager.cleanup_expired() }
                },
            ),
            spawn_shutdown_task(
                shutdown.clone(),
                shutdown_tx.subscribe(),
                Duration::from_secs(3600),
                move || {
                    let history_store = history_store_for_cleanup.clone();
                    async move { history_store.cleanup() }
                },
            ),
            spawn_shutdown_task(
                shutdown.clone(),
                shutdown_tx.subscribe(),
                Duration::from_millis(10),
                move || {
                    let node_prefetcher = node_prefetcher_for_processing.clone();
                    async move {
                        let _ = node_prefetcher.process_pending();
                    }
                },
            ),
            spawn_shutdown_task(
                shutdown,
                shutdown_tx.subscribe(),
                Duration::from_secs(1),
                move || {
                    let address_space = address_space_for_diagnostics.clone();
                    let session_manager = session_manager_for_diagnostics.clone();
                    let subscription_manager = subscription_manager_for_diagnostics.clone();
                    let security_manager = security_manager_for_diagnostics.clone();
                    async move {
                        address_space.refresh_diagnostics(&DiagnosticsSnapshot {
                            current_sessions: session_manager.session_count() as u32,
                            current_subscriptions: subscription_manager.subscription_count() as u32,
                            total_nodes: address_space.node_count() as u32,
                            namespace_count: address_space.namespaces().len() as u32,
                            security_profile_summary: security_manager.diagnostics_summary(),
                            durable_restore_summary: format!(
                                "{:?}/restored={}/detached={}",
                                subscription_manager.durability_mode(),
                                subscription_manager.restored_subscription_count(),
                                subscription_manager.detached_subscription_count()
                            ),
                            manager_ownership_summary: address_space
                                .manager_ownership_summary()
                                .join("; "),
                        });
                    }
                },
            ),
        ];

        Self {
            shutdown_tx,
            handles,
        }
    }

    async fn shutdown(self) {
        let _ = self.shutdown_tx.send(());
        for mut handle in self.handles {
            match tokio::time::timeout(Duration::from_secs(1), &mut handle).await {
                Ok(_) => {}
                Err(_) => {
                    handle.abort();
                }
            }
        }
    }
}

fn spawn_shutdown_task<F, Fut>(
    shutdown: Arc<AtomicBool>,
    mut shutdown_rx: broadcast::Receiver<()>,
    interval: Duration,
    action: F,
) -> JoinHandle<()>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            action().await;
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = shutdown_rx.recv() => break,
            }
        }
    })
}

fn build_transport_config(
    server_config: &OpcUaServerConfig,
    defaults: &ServerBuildDefaults,
) -> OpcUaResult<TransportAdapterConfig> {
    let (protocol, bind_address, endpoint_path) =
        parse_endpoint_url(server_config.endpoint_protocol, &server_config.endpoint_url)?;
    match (protocol, server_config.connection_mode) {
        (TransportProtocol::OpcTcp, TransportConnectionMode::Listener) => {
            Ok(TransportAdapterConfig::TcpListener(TcpTransportConfig {
                bind_address,
                max_connections: defaults.tcp.max_connections,
                connection_timeout: defaults.tcp.connection_timeout,
                server_buffer_size: defaults.tcp.server_buffer_size,
                initiation_mode: ConnectionInitiationMode::Listener,
            }))
        }
        (TransportProtocol::OpcTcp, TransportConnectionMode::ReverseConnect) => {
            let reverse_target =
                server_config
                    .reverse_connect_target
                    .as_ref()
                    .ok_or_else(|| {
                        OpcUaError::Config(
                            "reverse_connect transport requires reverse_connect_target".to_string(),
                        )
                    })?;
            let (target_protocol, target_address, _) =
                parse_endpoint_url(TransportProtocol::OpcTcp, reverse_target)?;
            if target_protocol != TransportProtocol::OpcTcp {
                return Err(OpcUaError::Config(
                    "reverse_connect only supports opc.tcp targets".to_string(),
                ));
            }
            if server_config.retry_interval_ms == 0 {
                return Err(OpcUaError::Config(
                    "reverse_connect requires retry_interval_ms > 0".to_string(),
                ));
            }
            Ok(TransportAdapterConfig::TcpReverse(
                TcpReverseConnectConfig {
                    target_address,
                    retry_interval: Duration::from_millis(server_config.retry_interval_ms),
                    connection_timeout: defaults.tcp.connection_timeout,
                    server_buffer_size: defaults.tcp.server_buffer_size,
                    initiation_mode: ConnectionInitiationMode::ReverseConnect,
                },
            ))
        }
        (TransportProtocol::Https, TransportConnectionMode::Listener) => {
            Ok(TransportAdapterConfig::Https(HttpsTransportConfig {
                bind_address,
                endpoint_path,
                max_connections: defaults.tcp.max_connections,
                connection_timeout: defaults.tcp.connection_timeout,
                initiation_mode: ConnectionInitiationMode::Listener,
                certificate_path: server_config.certificate_path.clone(),
                private_key_path: server_config.private_key_path.clone(),
            }))
        }
        (TransportProtocol::Https, TransportConnectionMode::ReverseConnect) => Err(
            OpcUaError::Config("https transport does not support reverse_connect".to_string()),
        ),
    }
}

fn build_internal_managers(
    catalog: Option<&GeneratedNodeCatalog>,
    #[cfg(feature = "experimental-namespace-api")] namespace_managers: &[Arc<
        dyn NamespaceManagerPlugin,
    >],
) -> Vec<Arc<dyn NodeManager>> {
    let mut managers: Vec<Arc<dyn NodeManager>> = Vec::new();

    if let Some(catalog) = catalog {
        let mut seen = BTreeSet::new();
        managers.extend(
            catalog
                .namespace_table
                .iter()
                .enumerate()
                .filter(|(index, uri)| {
                    *index > 0
                        && uri.as_str()
                            != crate::sdk::address_space::DiagnosticsNodeManager::NAMESPACE_URI
                        && seen.insert((*uri).clone())
                })
                .map(|(_, uri)| {
                    Arc::new(CatalogNodeManager::new(uri.clone())) as Arc<dyn NodeManager>
                }),
        );
    }

    #[cfg(feature = "experimental-namespace-api")]
    {
        managers.extend(
            namespace_managers
                .iter()
                .cloned()
                .map(adapt_namespace_manager_plugin),
        );
    }

    managers
}

fn parse_endpoint_url(
    transport_protocol: TransportProtocol,
    url: &str,
) -> OpcUaResult<(TransportProtocol, SocketAddr, String)> {
    let (protocol, addr_part) = if let Some(addr) = url.strip_prefix("opc.tcp://") {
        (TransportProtocol::OpcTcp, addr)
    } else if let Some(addr) = url.strip_prefix("https://") {
        (TransportProtocol::Https, addr)
    } else {
        (transport_protocol, url)
    };
    let path = {
        let path_start = addr_part.find('/').unwrap_or(addr_part.len());
        let raw = &addr_part[path_start..];
        if raw.is_empty() {
            "/".to_string()
        } else {
            raw.to_string()
        }
    };
    let addr_part = addr_part.split('/').next().unwrap_or(addr_part);

    if let Ok(socket_addr) = addr_part.parse::<SocketAddr>() {
        return Ok((protocol, socket_addr, path));
    }

    addr_part
        .to_socket_addrs()
        .map_err(|error| {
            OpcUaError::Server(format!(
                "Failed to resolve endpoint URL '{}' as socket address: {}",
                url, error
            ))
        })?
        .next()
        .map(|socket_addr| (protocol, socket_addr, path))
        .ok_or_else(|| {
            OpcUaError::Server(format!(
                "Failed to resolve endpoint URL '{}' as socket address",
                url
            ))
        })
}
