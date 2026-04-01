//! OPC UA subscription management.
//!
//! Subscriptions allow clients to receive notifications about data changes
//! and events without polling.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use std::time::Instant;

use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info};

use super::event::{EventFieldList, EventFilter};
use super::monitored_item::{MonitoredItem, MonitoredItemConfig, MonitoredItemNotification};
use crate::sdk::subscription::{
    DurableSubscriptionMetadata, DurableSubscriptionStatus, DurableSubscriptionStore,
    EventSubscriptionPort, MonitoredItemCatalog, NotificationQueue, PublishEngine,
    SubscriptionCatalog, SubscriptionDurabilityConfig, SubscriptionDurabilityMode,
    SubscriptionOwnershipState, SubscriptionRuntime, SubscriptionStateData,
};
use crate::types::{DataValue, NodeId};

/// Subscription configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SubscriptionConfig {
    /// Subscription ID (assigned by manager).
    pub subscription_id: u32,
    /// Publishing interval in milliseconds.
    pub publishing_interval_ms: f64,
    /// Lifetime count (max publish cycles without activity before deletion).
    pub lifetime_count: u32,
    /// Max keep-alive count (publish cycles without notifications before keep-alive).
    pub max_keep_alive_count: u32,
    /// Maximum notifications per publish response.
    pub max_notifications_per_publish: u32,
    /// Priority (higher = more important).
    pub priority: u8,
    /// Whether publishing is enabled.
    pub publishing_enabled: bool,
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        Self {
            subscription_id: 0,
            publishing_interval_ms: 1000.0,
            lifetime_count: 10_000,
            max_keep_alive_count: 10,
            max_notifications_per_publish: 1000,
            priority: 0,
            publishing_enabled: true,
        }
    }
}

impl SubscriptionConfig {
    /// Create with custom publishing interval.
    pub fn with_interval(interval_ms: f64) -> Self {
        Self {
            publishing_interval_ms: interval_ms,
            ..Default::default()
        }
    }

    /// Get the publishing interval as a Duration.
    pub fn publishing_interval(&self) -> Duration {
        Duration::from_micros((self.publishing_interval_ms * 1000.0) as u64)
    }
}

/// Subscription state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SubscriptionState {
    /// Subscription is creating.
    Creating,
    /// Subscription is normal (active).
    Normal,
    /// Subscription is in late state (no publish requests).
    Late,
    /// Subscription is in keep-alive state.
    KeepAlive,
    /// Subscription is closed.
    Closed,
}

/// A subscription instance.
pub struct Subscription {
    pub(crate) state_data: SubscriptionStateData,
    pub(crate) monitored_items: MonitoredItemCatalog,
    pub(crate) notification_queue: NotificationQueue,
    pub(crate) owner_session_id: Option<NodeId>,
    pub(crate) detached_restored: bool,
    #[allow(dead_code)]
    /// Creation time.
    pub(crate) created_at: DateTime<Utc>,
}

impl Subscription {
    /// Create a new subscription.
    pub fn new(config: SubscriptionConfig) -> Self {
        Self::new_with_ownership(config, SubscriptionOwnershipState::default())
    }

    pub(crate) fn new_with_ownership(
        config: SubscriptionConfig,
        ownership: SubscriptionOwnershipState,
    ) -> Self {
        Self {
            state_data: SubscriptionStateData::new(config),
            monitored_items: MonitoredItemCatalog::default(),
            notification_queue: NotificationQueue::default(),
            owner_session_id: ownership.owner_session_id,
            detached_restored: ownership.detached_restored,
            created_at: Utc::now(),
        }
    }

    /// Get subscription ID.
    pub fn id(&self) -> u32 {
        self.state_data.config.subscription_id
    }

    /// Get current state.
    pub fn state(&self) -> SubscriptionState {
        self.state_data.state
    }

    /// Set state.
    pub fn set_state(&mut self, state: SubscriptionState) {
        self.state_data.state = state;
    }

    /// Get configuration.
    pub fn config(&self) -> &SubscriptionConfig {
        &self.state_data.config
    }

    /// Modify configuration.
    pub fn modify(
        &mut self,
        publishing_interval_ms: f64,
        lifetime_count: u32,
        max_keep_alive_count: u32,
    ) {
        self.state_data.config.publishing_interval_ms = publishing_interval_ms;
        self.state_data.config.lifetime_count = lifetime_count;
        self.state_data.config.max_keep_alive_count = max_keep_alive_count;
    }

    /// Enable/disable publishing.
    pub fn set_publishing_enabled(&mut self, enabled: bool) {
        self.state_data.config.publishing_enabled = enabled;
    }

    /// Create a monitored item.
    pub fn create_monitored_item(&mut self, config: MonitoredItemConfig) -> u32 {
        self.monitored_items.create(&mut self.state_data, config)
    }

    /// Delete a monitored item.
    pub fn delete_monitored_item(&mut self, item_id: u32) -> bool {
        self.monitored_items.delete(item_id)
    }

    /// Get a monitored item.
    pub fn get_monitored_item(&self, item_id: u32) -> Option<&MonitoredItem> {
        self.monitored_items.get(item_id)
    }

    /// Get a mutable monitored item.
    pub fn get_monitored_item_mut(&mut self, item_id: u32) -> Option<&mut MonitoredItem> {
        self.monitored_items.get_mut(item_id)
    }

    /// Get all monitored item IDs.
    pub fn monitored_item_ids(&self) -> Vec<u32> {
        self.monitored_items.ids()
    }

    /// Get monitored item count.
    pub fn monitored_item_count(&self) -> usize {
        self.monitored_items.len()
    }

    /// Process a value change for a node.
    pub fn on_value_change(&mut self, node_id: &NodeId, value: DataValue) {
        self.monitored_items
            .on_value_change(node_id, value, &mut self.notification_queue);
    }

    /// Process pending notifications (both data change and events).
    pub fn process_publish(&mut self) -> Option<NotificationMessage> {
        PublishEngine::process_publish(self)
    }

    /// Push an event notification into the pending event queue.
    pub fn push_event_notification(&mut self, field_list: EventFieldList) {
        self.notification_queue.push_event_notification(
            self.state_data.config.max_notifications_per_publish,
            field_list,
        );
    }

    /// Get event-monitoring items with their filters.
    ///
    /// Returns a list of (client_handle, EventFilter) for items that have event monitoring enabled.
    pub fn get_event_monitored_items(&self) -> Vec<(u32, EventFilter)> {
        self.monitored_items.event_monitored_items()
    }

    /// Tick the subscription (advance lifetime counter).
    pub fn tick(&mut self) {
        self.state_data.tick();
    }

    /// Check if subscription should be deleted.
    pub fn should_delete(&self) -> bool {
        self.state_data.state == SubscriptionState::Closed
    }

    pub(crate) fn ownership_state(&self) -> SubscriptionOwnershipState {
        SubscriptionOwnershipState {
            owner_session_id: self.owner_session_id.clone(),
            detached_restored: self.detached_restored,
        }
    }

    pub(crate) fn attach_owner(&mut self, session_id: NodeId) {
        self.owner_session_id = Some(session_id);
        self.detached_restored = false;
    }

    pub(crate) fn is_publishable(&self) -> bool {
        !self.detached_restored
    }

    pub(crate) fn sampling_bucket_ms(&self) -> u64 {
        self.monitored_items
            .ids()
            .into_iter()
            .filter_map(|item_id| {
                self.monitored_items
                    .get(item_id)
                    .map(|item| item.config().sampling_interval_ms.max(1.0) as u64)
            })
            .min()
            .unwrap_or_else(|| self.state_data.config.publishing_interval_ms.max(1.0) as u64)
    }
}

/// Notification message sent to clients.
#[derive(Debug, Clone)]
pub struct NotificationMessage {
    /// Subscription ID.
    pub subscription_id: u32,
    /// Sequence number.
    pub sequence_number: u32,
    /// Publish time.
    pub publish_time: DateTime<Utc>,
    /// Data change notifications.
    pub notifications: Vec<MonitoredItemNotification>,
    /// Event notifications (EventFieldList entries).
    pub event_notifications: Vec<EventFieldList>,
    /// More notifications available.
    pub more_notifications: bool,
}

/// Publish response sent to clients.
#[derive(Debug, Clone)]
pub struct PublishResponse {
    /// Subscription ID.
    pub subscription_id: u32,
    /// Available sequence numbers.
    pub available_sequence_numbers: Vec<u32>,
    /// Notification message (if any).
    pub notification_message: Option<NotificationMessage>,
    /// More notifications available.
    pub more_notifications: bool,
}

/// Subscription event.
#[derive(Debug, Clone)]
pub enum SubscriptionEvent {
    /// Subscription created.
    Created { subscription_id: u32 },
    /// Subscription modified.
    Modified { subscription_id: u32 },
    /// Subscription deleted.
    Deleted { subscription_id: u32 },
    /// Notification sent.
    Notification(NotificationMessage),
}

#[derive(Debug, Clone)]
pub(crate) struct TransferSubscriptionOutcome {
    pub(crate) previous_owner_session_id: Option<NodeId>,
    pub(crate) available_sequence_numbers: Vec<u32>,
}

/// Subscription manager configuration.
#[derive(Debug, Clone)]
pub struct SubscriptionManagerConfig {
    /// Maximum number of subscriptions.
    pub max_subscriptions: usize,
    /// Maximum monitored items per subscription.
    pub max_monitored_items_per_subscription: usize,
    /// Notification channel buffer size.
    pub notification_buffer_size: usize,
    /// Event channel buffer size.
    pub event_buffer_size: usize,
    /// Runtime durability configuration.
    pub durability: SubscriptionDurabilityConfig,
}

impl Default for SubscriptionManagerConfig {
    fn default() -> Self {
        Self {
            max_subscriptions: 10_000,
            max_monitored_items_per_subscription: 100_000,
            notification_buffer_size: 10_000,
            event_buffer_size: 1000,
            durability: SubscriptionDurabilityConfig::default(),
        }
    }
}

/// Subscription manager.
///
/// Manages all subscriptions and their monitored items.
pub struct SubscriptionManager {
    config: SubscriptionManagerConfig,
    catalog: SubscriptionCatalog,
    runtime: SubscriptionRuntime,
    event_tx: broadcast::Sender<SubscriptionEvent>,
    notification_tx: mpsc::Sender<NotificationMessage>,
    notification_rx: RwLock<Option<mpsc::Receiver<NotificationMessage>>>,
    durable_store: Option<DurableSubscriptionStore>,
    durable_dirty: AtomicBool,
    last_durable_flush: Mutex<Instant>,
    last_durable_flush_at: Mutex<Option<DateTime<Utc>>>,
    last_durable_flush_result: RwLock<String>,
    restored_subscription_count: AtomicUsize,
    detached_subscription_count: AtomicUsize,
}

impl SubscriptionManager {
    /// Create a new subscription manager with default config.
    pub fn new() -> Self {
        Self::with_config(SubscriptionManagerConfig::default())
    }

    /// Create with legacy parameters.
    pub fn with_params(
        max_subscriptions: usize,
        max_monitored_items_per_subscription: usize,
    ) -> Self {
        Self::with_config(SubscriptionManagerConfig {
            max_subscriptions,
            max_monitored_items_per_subscription,
            ..Default::default()
        })
    }

    /// Create a new subscription manager with config.
    pub fn with_config(config: SubscriptionManagerConfig) -> Self {
        let (event_tx, _) = broadcast::channel(config.event_buffer_size);
        let (notification_tx, notification_rx) = mpsc::channel(config.notification_buffer_size);
        let durable_store = DurableSubscriptionStore::new(config.durability.clone());

        let manager = Self {
            config,
            catalog: SubscriptionCatalog::new(),
            runtime: SubscriptionRuntime::new(),
            event_tx,
            notification_tx,
            notification_rx: RwLock::new(Some(notification_rx)),
            durable_store,
            durable_dirty: AtomicBool::new(false),
            last_durable_flush: Mutex::new(Instant::now()),
            last_durable_flush_at: Mutex::new(None),
            last_durable_flush_result: RwLock::new("never_flushed".to_string()),
            restored_subscription_count: AtomicUsize::new(0),
            detached_subscription_count: AtomicUsize::new(0),
        };

        if let Some(store) = &manager.durable_store {
            if store.config().restore_on_start {
                if let Ok(snapshot) = store.load() {
                    let restored = snapshot.subscriptions.len();
                    manager.catalog.replace_from_snapshot(snapshot);
                    manager
                        .restored_subscription_count
                        .store(restored, Ordering::Relaxed);
                    manager.detached_subscription_count.store(
                        manager.catalog.detached_subscription_count(),
                        Ordering::Relaxed,
                    );
                }
            }
        }

        manager
    }

    fn mark_durable_dirty(&self) {
        if self.config.durability.mode == SubscriptionDurabilityMode::Persisted {
            self.durable_dirty.store(true, Ordering::Relaxed);
        }
    }

    fn persist_if_due(&self, force: bool) {
        let Some(store) = &self.durable_store else {
            return;
        };
        if !force && !self.durable_dirty.load(Ordering::Relaxed) {
            return;
        }

        let elapsed = self.last_durable_flush.lock().elapsed();
        if !force && elapsed < Duration::from_millis(self.config.durability.flush_interval_ms) {
            return;
        }

        let metadata = DurableSubscriptionMetadata {
            saved_at: Some(Utc::now()),
            last_flush_result: "ok".to_string(),
        };
        if let Err(error) = store.save_catalog(&self.catalog, metadata) {
            *self.last_durable_flush_result.write() = format!("error: {}", error);
            tracing::warn!(error = %error, "failed to persist OPC UA subscription state");
            return;
        }

        *self.last_durable_flush.lock() = Instant::now();
        *self.last_durable_flush_at.lock() = Some(Utc::now());
        *self.last_durable_flush_result.write() = "ok".to_string();
        self.durable_dirty.store(false, Ordering::Relaxed);
    }

    fn refresh_detached_count(&self) {
        self.detached_subscription_count.store(
            self.catalog.detached_subscription_count(),
            Ordering::Relaxed,
        );
    }

    pub fn durability_mode(&self) -> SubscriptionDurabilityMode {
        self.config.durability.mode
    }

    pub fn restored_subscription_count(&self) -> usize {
        self.restored_subscription_count.load(Ordering::Relaxed)
    }

    pub fn detached_subscription_count(&self) -> usize {
        self.detached_subscription_count.load(Ordering::Relaxed)
    }

    pub(crate) fn durability_status(&self) -> DurableSubscriptionStatus {
        if let Some(store) = &self.durable_store {
            if let Ok(status) = store.load_status() {
                return status;
            }
        }
        DurableSubscriptionStatus {
            persisted_state_present: false,
            restored_subscription_count: self.restored_subscription_count(),
            detached_subscription_count: self.detached_subscription_count(),
            last_flush_at: *self.last_durable_flush_at.lock(),
            last_flush_result: self.last_durable_flush_result.read().clone(),
        }
    }

    pub fn clear_persisted_state(&self) {
        if let Some(store) = &self.durable_store {
            if let Err(error) = store.clear() {
                tracing::warn!(error = %error, "failed to clear OPC UA subscription state");
                *self.last_durable_flush_result.write() = format!("error: {}", error);
            } else {
                *self.last_durable_flush_result.write() = "cleared".to_string();
                *self.last_durable_flush_at.lock() = Some(Utc::now());
            }
        }
    }

    /// Create a subscription (shorthand).
    pub fn create(&self, config: SubscriptionConfig) -> Result<u32, SubscriptionError> {
        self.create_subscription(config)
    }

    /// Get a subscription by ID.
    pub fn get(&self, subscription_id: u32) -> Option<SubscriptionConfig> {
        self.catalog
            .get(subscription_id)
            .map(|sub| sub.read().config().clone())
    }

    /// Create a subscription.
    pub fn create_subscription(
        &self,
        config: SubscriptionConfig,
    ) -> Result<u32, SubscriptionError> {
        self.create_subscription_for_owner(config, None)
    }

    pub(crate) fn create_subscription_for_owner(
        &self,
        config: SubscriptionConfig,
        owner_session_id: Option<NodeId>,
    ) -> Result<u32, SubscriptionError> {
        let id = self.catalog.create_subscription_for_owner(
            self.config.max_subscriptions,
            config,
            owner_session_id,
        )?;

        info!(subscription_id = id, "Subscription created");
        let _ = self.event_tx.send(SubscriptionEvent::Created {
            subscription_id: id,
        });
        self.mark_durable_dirty();
        self.persist_if_due(true);
        self.refresh_detached_count();

        Ok(id)
    }

    /// Delete a subscription.
    pub fn delete_subscription(&self, subscription_id: u32) -> bool {
        if self.catalog.remove(subscription_id) {
            info!(subscription_id, "Subscription deleted");
            let _ = self
                .event_tx
                .send(SubscriptionEvent::Deleted { subscription_id });
            self.mark_durable_dirty();
            self.persist_if_due(true);
            self.refresh_detached_count();
            true
        } else {
            false
        }
    }

    /// Modify a subscription.
    pub fn modify_subscription(
        &self,
        subscription_id: u32,
        publishing_interval_ms: f64,
        lifetime_count: u32,
        max_keep_alive_count: u32,
    ) -> Result<(), SubscriptionError> {
        let subscription = self
            .catalog
            .get(subscription_id)
            .ok_or(SubscriptionError::SubscriptionNotFound)?;

        {
            let mut sub = subscription.write();
            sub.modify(publishing_interval_ms, lifetime_count, max_keep_alive_count);
        }

        let _ = self
            .event_tx
            .send(SubscriptionEvent::Modified { subscription_id });
        self.mark_durable_dirty();
        self.persist_if_due(true);
        Ok(())
    }

    /// Create a monitored item.
    pub fn create_monitored_item(
        &self,
        subscription_id: u32,
        config: MonitoredItemConfig,
    ) -> Result<u32, SubscriptionError> {
        let subscription = self
            .catalog
            .get(subscription_id)
            .ok_or(SubscriptionError::SubscriptionNotFound)?;

        let item_id = {
            let mut sub = subscription.write();

            if sub.monitored_item_count() >= self.config.max_monitored_items_per_subscription {
                return Err(SubscriptionError::TooManyMonitoredItems);
            }

            sub.create_monitored_item(config)
        };

        debug!(subscription_id, item_id, "Monitored item created");
        self.mark_durable_dirty();
        self.persist_if_due(true);
        self.refresh_detached_count();
        Ok(item_id)
    }

    /// Modify a monitored item's parameters.
    pub fn modify_monitored_item(
        &self,
        subscription_id: u32,
        item_id: u32,
        sampling_interval_ms: f64,
        queue_size: u32,
        discard_oldest: bool,
    ) -> Result<(), SubscriptionError> {
        let subscription = self
            .catalog
            .get(subscription_id)
            .ok_or(SubscriptionError::SubscriptionNotFound)?;

        {
            let mut sub = subscription.write();
            let item = sub
                .get_monitored_item_mut(item_id)
                .ok_or(SubscriptionError::MonitoredItemNotFound)?;

            item.modify(sampling_interval_ms, queue_size, discard_oldest, None);
        }
        debug!(subscription_id, item_id, "Monitored item modified");
        self.mark_durable_dirty();
        self.persist_if_due(true);
        Ok(())
    }

    /// Delete a monitored item.
    pub fn delete_monitored_item(
        &self,
        subscription_id: u32,
        item_id: u32,
    ) -> Result<bool, SubscriptionError> {
        let subscription = self
            .catalog
            .get(subscription_id)
            .ok_or(SubscriptionError::SubscriptionNotFound)?;

        let deleted = {
            let mut sub = subscription.write();
            sub.delete_monitored_item(item_id)
        };
        if deleted {
            self.mark_durable_dirty();
            self.persist_if_due(true);
            self.refresh_detached_count();
        }
        Ok(deleted)
    }

    /// Process a value change.
    pub async fn on_value_change(&self, node_id: &NodeId, value: DataValue) {
        self.catalog.for_each_mut(|subscription| {
            subscription.on_value_change(node_id, value.clone());
        });
        self.mark_durable_dirty();
    }

    /// Process publish for a subscription.
    pub fn process_publish(&self, subscription_id: u32) -> Option<PublishResponse> {
        let subscription = self.catalog.get(subscription_id)?;
        let mut sub = subscription.write();
        if !sub.is_publishable() {
            return None;
        }
        Some(PublishEngine::build_publish_response(
            subscription_id,
            &mut sub,
        ))
    }

    /// Process all subscriptions (called periodically).
    pub async fn process_all(&self) {
        let changed = self
            .runtime
            .tick_all(&self.catalog, &self.notification_tx, &self.event_tx)
            .await;
        if changed {
            self.mark_durable_dirty();
        }
        self.refresh_detached_count();
        self.persist_if_due(false);
    }

    /// Get subscription count.
    pub fn subscription_count(&self) -> usize {
        self.catalog.len()
    }

    /// Get all subscription IDs.
    pub fn subscription_ids(&self) -> Vec<u32> {
        self.catalog.ids()
    }

    /// Subscribe to events.
    pub fn subscribe_events(&self) -> broadcast::Receiver<SubscriptionEvent> {
        self.event_tx.subscribe()
    }

    /// Take the notification receiver.
    pub fn take_notification_receiver(&self) -> Option<mpsc::Receiver<NotificationMessage>> {
        self.notification_rx.write().take()
    }

    /// Get event-monitoring items for a subscription.
    ///
    /// Returns a list of (client_handle, EventFilter) for items that have event monitoring enabled.
    pub fn get_event_monitored_items(&self, subscription_id: u32) -> Vec<(u32, EventFilter)> {
        match self.catalog.get(subscription_id) {
            Some(sub) => sub.read().get_event_monitored_items(),
            None => Vec::new(),
        }
    }

    /// Push an event notification into a subscription's pending event queue.
    pub fn push_event_notification(&self, subscription_id: u32, field_list: EventFieldList) {
        if let Some(sub) = self.catalog.get(subscription_id) {
            sub.write().push_event_notification(field_list);
            self.mark_durable_dirty();
        }
    }

    pub(crate) fn subscription_ownership(
        &self,
        subscription_id: u32,
    ) -> Option<SubscriptionOwnershipState> {
        self.catalog.ownership_state(subscription_id)
    }

    pub(crate) fn transfer_subscription(
        &self,
        subscription_id: u32,
        owner_session_id: NodeId,
    ) -> Result<TransferSubscriptionOutcome, SubscriptionError> {
        let previous = self
            .catalog
            .transfer_subscription(subscription_id, owner_session_id)?;
        self.mark_durable_dirty();
        self.persist_if_due(true);
        self.refresh_detached_count();
        Ok(TransferSubscriptionOutcome {
            previous_owner_session_id: previous.owner_session_id,
            available_sequence_numbers: self
                .catalog
                .get(subscription_id)
                .map(|subscription| vec![subscription.read().state_data.sequence_number])
                .unwrap_or_default(),
        })
    }
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSubscriptionPort for SubscriptionManager {
    fn subscription_ids(&self) -> Vec<u32> {
        SubscriptionManager::subscription_ids(self)
    }

    fn get_event_monitored_items(&self, subscription_id: u32) -> Vec<(u32, EventFilter)> {
        SubscriptionManager::get_event_monitored_items(self, subscription_id)
    }

    fn push_event_notification(&self, subscription_id: u32, field_list: EventFieldList) {
        SubscriptionManager::push_event_notification(self, subscription_id, field_list)
    }
}

/// Subscription error types.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SubscriptionError {
    #[error("Maximum subscriptions reached")]
    MaxSubscriptionsReached,
    #[error("Subscription not found")]
    SubscriptionNotFound,
    #[error("Monitored item not found")]
    MonitoredItemNotFound,
    #[error("Too many monitored items")]
    TooManyMonitoredItems,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Variant;
    use tempfile::tempdir;

    #[test]
    fn test_create_subscription() {
        let manager = SubscriptionManager::default();

        let id = manager
            .create_subscription(SubscriptionConfig::default())
            .unwrap();
        assert!(id > 0);
        assert_eq!(manager.subscription_count(), 1);
    }

    #[test]
    fn test_delete_subscription() {
        let manager = SubscriptionManager::default();

        let id = manager
            .create_subscription(SubscriptionConfig::default())
            .unwrap();
        assert!(manager.delete_subscription(id));
        assert_eq!(manager.subscription_count(), 0);
    }

    #[test]
    fn test_create_monitored_item() {
        let manager = SubscriptionManager::default();

        let sub_id = manager
            .create_subscription(SubscriptionConfig::default())
            .unwrap();

        let item_config = MonitoredItemConfig {
            node_id: NodeId::numeric(2, 1001),
            attribute_id: crate::types::AttributeId::Value,
            sampling_interval_ms: 1000.0,
            queue_size: 10,
            discard_oldest: true,
            filter: None,
            ..Default::default()
        };

        let item_id = manager.create_monitored_item(sub_id, item_config).unwrap();
        assert!(item_id > 0);
    }

    #[test]
    fn test_max_subscriptions() {
        let manager = SubscriptionManager::with_params(2, 100);

        manager
            .create_subscription(SubscriptionConfig::default())
            .unwrap();
        manager
            .create_subscription(SubscriptionConfig::default())
            .unwrap();

        let result = manager.create_subscription(SubscriptionConfig::default());
        assert!(matches!(
            result,
            Err(SubscriptionError::MaxSubscriptionsReached)
        ));
    }

    #[test]
    fn test_subscription_notification() {
        let mut subscription = Subscription::new(SubscriptionConfig::default());

        let item_config = MonitoredItemConfig {
            node_id: NodeId::numeric(2, 1001),
            attribute_id: crate::types::AttributeId::Value,
            sampling_interval_ms: 1000.0,
            queue_size: 10,
            discard_oldest: true,
            filter: None,
            ..Default::default()
        };

        subscription.create_monitored_item(item_config);

        // Trigger value change
        subscription.on_value_change(
            &NodeId::numeric(2, 1001),
            DataValue::new(Variant::double(25.5)),
        );

        // Process publish
        let message = subscription.process_publish().unwrap();
        assert_eq!(message.notifications.len(), 1);
    }

    #[test]
    fn persisted_durability_restores_subscriptions() {
        let state_dir = tempdir().unwrap();
        let config = SubscriptionManagerConfig {
            durability: SubscriptionDurabilityConfig {
                mode: SubscriptionDurabilityMode::Persisted,
                state_dir: Some(state_dir.path().to_path_buf()),
                flush_interval_ms: 0,
                restore_on_start: true,
            },
            ..Default::default()
        };
        let manager = SubscriptionManager::with_config(config.clone());
        let owner_session = NodeId::string(1, "session-a");
        let subscription_id = manager
            .create_subscription_for_owner(SubscriptionConfig::default(), Some(owner_session))
            .unwrap();
        let item_config = MonitoredItemConfig {
            node_id: NodeId::numeric(2, 1001),
            attribute_id: crate::types::AttributeId::Value,
            sampling_interval_ms: 1000.0,
            queue_size: 10,
            discard_oldest: true,
            filter: None,
            ..Default::default()
        };
        let _ = manager
            .create_monitored_item(subscription_id, item_config)
            .unwrap();

        let restored = SubscriptionManager::with_config(config);
        assert_eq!(restored.subscription_count(), 1);
        assert_eq!(restored.restored_subscription_count(), 1);
        assert_eq!(restored.detached_subscription_count(), 1);
        assert!(restored.process_publish(subscription_id).is_none());

        let transfer = restored
            .transfer_subscription(subscription_id, NodeId::string(1, "session-b"))
            .unwrap();
        assert_eq!(transfer.available_sequence_numbers.len(), 1);
        assert_eq!(restored.detached_subscription_count(), 0);
        assert!(restored.process_publish(subscription_id).is_some());
    }
}
