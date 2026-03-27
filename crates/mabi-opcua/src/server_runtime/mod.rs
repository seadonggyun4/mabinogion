mod methods;

use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::info;

use crate::config::OpcUaServerConfig;
use crate::core::services::BuiltinServiceSet;
use crate::error::{OpcUaError, OpcUaResult};
use crate::modeling::GeneratedNodeCatalog;
use crate::nodes::{
    AddressSpace, AddressSpaceConfig, NodeCache, NodeCacheConfig, NodePrefetcher, PrefetchConfig,
};
use crate::sdk::history::{HistoryStore, HistoryStoreConfig};
use crate::sdk::session::{SessionManager, SessionManagerConfig};
use crate::sdk::subscription::{SubscriptionManager, SubscriptionManagerConfig};
use crate::security::{SecurityManager, SecurityManagerConfig};
use crate::transport::connection::ServiceContextTemplate;
use crate::transport::hooks::TransportHooks;
use crate::transport::runtime::{TransportRuntime, TransportRuntimePolicy};
use crate::transport::tcp_listener::{OpcUaTcpListener, TcpTransportConfig};

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
            security: SecurityManagerConfig::default(),
            tcp: TcpTransportConfig {
                bind_address: SocketAddr::from(([0, 0, 0, 0], 4840)),
                max_connections: 1000,
                connection_timeout: Duration::from_secs(60),
                server_buffer_size: 65_535,
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
        }
    }

    pub(crate) fn with_generated_catalog(
        mut self,
        generated_catalog: GeneratedNodeCatalog,
    ) -> Self {
        self.generated_catalog = Some(generated_catalog);
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
        let address_space = Arc::new(AddressSpace::new(spec.defaults.address_space.clone()));
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

        let tcp_config = TcpTransportConfig {
            bind_address: parse_endpoint_url(&spec.server_config.endpoint_url)?,
            max_connections: spec.defaults.tcp.max_connections,
            connection_timeout: spec.transport_policy.connection_timeout,
            server_buffer_size: spec.transport_policy.server_buffer_size,
        };

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
            builtins,
            tcp_config,
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
    builtins: Arc<BuiltinServiceSet>,
    tcp_config: TcpTransportConfig,
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
            builtins: self.builtins,
            tcp_config: self.tcp_config,
            transport_runtime: self.transport_runtime,
            tcp_listener: RwLock::new(None),
            tcp_listener_task: RwLock::new(None),
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
    #[allow(dead_code)]
    builtins: Arc<BuiltinServiceSet>,
    tcp_config: TcpTransportConfig,
    transport_runtime: Arc<TransportRuntime>,
    tcp_listener: RwLock<Option<Arc<OpcUaTcpListener>>>,
    tcp_listener_task: RwLock<Option<JoinHandle<()>>>,
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
        if self.tcp_listener.read().is_some() {
            return Ok(());
        }

        let tcp_listener = Arc::new(OpcUaTcpListener::new(
            self.tcp_config.clone(),
            self.transport_runtime.clone(),
        ));
        let listener_task = {
            let tcp_listener = tcp_listener.clone();
            tokio::spawn(async move {
                if let Err(error) = tcp_listener.run().await {
                    tracing::warn!(error = %error, "TCP listener error");
                }
            })
        };

        shutdown.store(false, Ordering::Relaxed);
        *self.tcp_listener.write() = Some(tcp_listener);
        *self.tcp_listener_task.write() = Some(listener_task);
        Ok(())
    }

    pub(crate) async fn stop_transport(&self) {
        if let Some(listener) = self.tcp_listener.read().as_ref() {
            listener.shutdown();
        }

        let listener_task = self.tcp_listener_task.write().take();
        if let Some(listener_task) = listener_task {
            let _ = tokio::time::timeout(Duration::from_secs(5), listener_task).await;
        }
        self.tcp_listener.write().take();
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
            cache_hit_rate: cache_stats.hit_rate(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ServerStatsInputs {
    pub(crate) active_sessions: usize,
    pub(crate) active_subscriptions: usize,
    pub(crate) total_nodes: usize,
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
        shutdown: Arc<AtomicBool>,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let handles = vec![
            spawn_shutdown_task(
                shutdown.clone(),
                shutdown_tx.subscribe(),
                Duration::from_millis(100),
                move || {
                    let subscription_manager = subscription_manager.clone();
                    async move { subscription_manager.process_all().await }
                },
            ),
            spawn_shutdown_task(
                shutdown.clone(),
                shutdown_tx.subscribe(),
                Duration::from_secs(10),
                move || {
                    let session_manager = session_manager.clone();
                    async move { session_manager.cleanup_expired() }
                },
            ),
            spawn_shutdown_task(
                shutdown.clone(),
                shutdown_tx.subscribe(),
                Duration::from_secs(3600),
                move || {
                    let history_store = history_store.clone();
                    async move { history_store.cleanup() }
                },
            ),
            spawn_shutdown_task(
                shutdown,
                shutdown_tx.subscribe(),
                Duration::from_millis(10),
                move || {
                    let node_prefetcher = node_prefetcher.clone();
                    async move {
                        let _ = node_prefetcher.process_pending();
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

fn parse_endpoint_url(url: &str) -> OpcUaResult<SocketAddr> {
    let addr_part = url.strip_prefix("opc.tcp://").unwrap_or(url);
    let addr_part = addr_part.split('/').next().unwrap_or(addr_part);

    if let Ok(socket_addr) = addr_part.parse::<SocketAddr>() {
        return Ok(socket_addr);
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
        .ok_or_else(|| {
            OpcUaError::Server(format!(
                "Failed to resolve endpoint URL '{}' as socket address",
                url
            ))
        })
}
