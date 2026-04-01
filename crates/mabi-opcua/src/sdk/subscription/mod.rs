use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};

use crate::types::{DataValue, NodeId};

pub(crate) mod event {
    pub(crate) use crate::sdk::event::{EventFieldList, EventFilter};
}

pub(crate) mod monitored_item;
pub(crate) mod public;

pub use monitored_item::{
    DataChangeFilter, DataChangeTrigger, DeadbandType, MonitoredItem, MonitoredItemConfig,
    MonitoredItemKind, MonitoredItemNotification, MonitoringMode,
};
pub use public::{
    NotificationMessage, PublishResponse, Subscription, SubscriptionConfig, SubscriptionError,
    SubscriptionEvent, SubscriptionManager, SubscriptionManagerConfig, SubscriptionState,
};

use crate::sdk::event::{EventFieldList, EventFilter};
use crate::sdk::subscription::monitored_item::PersistedMonitoredItem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionDurabilityMode {
    Ephemeral,
    #[default]
    SessionOnly,
    Persisted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionDurabilityConfig {
    #[serde(default)]
    pub mode: SubscriptionDurabilityMode,
    #[serde(default)]
    pub state_dir: Option<PathBuf>,
    #[serde(default = "default_flush_interval_ms")]
    pub flush_interval_ms: u64,
    #[serde(default = "default_true")]
    pub restore_on_start: bool,
}

fn default_flush_interval_ms() -> u64 {
    1_000
}

fn default_true() -> bool {
    true
}

impl Default for SubscriptionDurabilityConfig {
    fn default() -> Self {
        Self {
            mode: SubscriptionDurabilityMode::SessionOnly,
            state_dir: None,
            flush_interval_ms: default_flush_interval_ms(),
            restore_on_start: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DurableSubscriptionMetadata {
    #[serde(default)]
    pub(crate) saved_at: Option<DateTime<Utc>>,
    #[serde(default = "default_flush_result")]
    pub(crate) last_flush_result: String,
}

fn default_flush_result() -> String {
    "never_flushed".to_string()
}

impl Default for DurableSubscriptionMetadata {
    fn default() -> Self {
        Self {
            saved_at: None,
            last_flush_result: default_flush_result(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct DurableSubscriptionSnapshot {
    #[serde(default)]
    pub(crate) metadata: DurableSubscriptionMetadata,
    #[serde(default)]
    pub(crate) subscriptions: Vec<PersistedSubscriptionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct PersistedSubscriptionOwnership {
    #[serde(default)]
    pub(crate) owner_session_id: Option<NodeId>,
    #[serde(default)]
    pub(crate) detached_restored: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedSubscriptionRecord {
    pub(crate) subscription_id: u32,
    pub(crate) state_data: SubscriptionStateData,
    pub(crate) monitored_items: Vec<PersistedMonitoredItem>,
    pub(crate) pending_notifications: Vec<MonitoredItemNotification>,
    pub(crate) pending_event_notifications: Vec<EventFieldList>,
    #[serde(default)]
    pub(crate) ownership: PersistedSubscriptionOwnership,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct DurableSubscriptionStatus {
    pub(crate) persisted_state_present: bool,
    pub(crate) restored_subscription_count: usize,
    pub(crate) detached_subscription_count: usize,
    pub(crate) last_flush_at: Option<DateTime<Utc>>,
    pub(crate) last_flush_result: String,
}

pub(crate) struct DurableSubscriptionStore {
    config: SubscriptionDurabilityConfig,
    state_file: PathBuf,
}

impl DurableSubscriptionStore {
    pub(crate) fn new(config: SubscriptionDurabilityConfig) -> Option<Self> {
        if config.mode != SubscriptionDurabilityMode::Persisted {
            return None;
        }
        let state_dir = config
            .state_dir
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("mabi-opcua-subscriptions"));
        Some(Self {
            state_file: state_dir.join("subscriptions.json"),
            config,
        })
    }

    pub(crate) fn config(&self) -> &SubscriptionDurabilityConfig {
        &self.config
    }

    pub(crate) fn clear(&self) -> std::io::Result<()> {
        match fs::remove_file(&self.state_file) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn load(&self) -> std::io::Result<DurableSubscriptionSnapshot> {
        if !self.state_file.exists() {
            return Ok(DurableSubscriptionSnapshot::default());
        }
        let content = fs::read_to_string(&self.state_file)?;
        serde_json::from_str(&content).map_err(std::io::Error::other)
    }

    pub(crate) fn load_status(&self) -> std::io::Result<DurableSubscriptionStatus> {
        let snapshot = self.load()?;
        Ok(DurableSubscriptionStatus {
            persisted_state_present: self.state_file.exists(),
            restored_subscription_count: snapshot.subscriptions.len(),
            detached_subscription_count: snapshot
                .subscriptions
                .iter()
                .filter(|record| record.ownership.detached_restored)
                .count(),
            last_flush_at: snapshot.metadata.saved_at,
            last_flush_result: snapshot.metadata.last_flush_result,
        })
    }

    pub(crate) fn save_catalog(
        &self,
        catalog: &SubscriptionCatalog,
        metadata: DurableSubscriptionMetadata,
    ) -> std::io::Result<()> {
        let snapshot = catalog.snapshot(metadata);
        let content = serde_json::to_string_pretty(&snapshot).map_err(std::io::Error::other)?;
        if let Some(parent) = self.state_file.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp_file = self.state_file.with_extension("json.tmp");
        fs::write(&temp_file, content)?;
        fs::rename(temp_file, &self.state_file)?;
        Ok(())
    }
}

pub(crate) trait EventSubscriptionPort: Send + Sync {
    fn subscription_ids(&self) -> Vec<u32>;
    fn get_event_monitored_items(&self, subscription_id: u32) -> Vec<(u32, EventFilter)>;
    fn push_event_notification(&self, subscription_id: u32, field_list: EventFieldList);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SubscriptionStateData {
    pub(crate) config: SubscriptionConfig,
    pub(crate) state: SubscriptionState,
    pub(crate) next_item_id: u32,
    pub(crate) sequence_number: u32,
    pub(crate) keep_alive_counter: u32,
    pub(crate) lifetime_counter: u32,
    pub(crate) last_publish_time: DateTime<Utc>,
}

impl SubscriptionStateData {
    pub(crate) fn new(config: SubscriptionConfig) -> Self {
        Self {
            config,
            state: SubscriptionState::Creating,
            next_item_id: 1,
            sequence_number: 1,
            keep_alive_counter: 0,
            lifetime_counter: 0,
            last_publish_time: Utc::now(),
        }
    }
    pub(crate) fn tick(&mut self) {
        self.lifetime_counter += 1;
        if self.lifetime_counter >= self.config.lifetime_count {
            self.state = SubscriptionState::Closed;
        }
    }

    pub(crate) fn reset_lifetime(&mut self) {
        self.lifetime_counter = 0;
    }

    pub(crate) fn mark_keepalive(&mut self) {
        self.keep_alive_counter = 0;
        self.state = SubscriptionState::KeepAlive;
    }

    pub(crate) fn mark_published(&mut self) {
        self.sequence_number = self.sequence_number.wrapping_add(1);
        self.keep_alive_counter = 0;
        self.state = SubscriptionState::Normal;
        self.last_publish_time = Utc::now();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct SubscriptionOwnershipState {
    pub(crate) owner_session_id: Option<NodeId>,
    #[serde(default)]
    pub(crate) detached_restored: bool,
}

#[derive(Default)]
pub(crate) struct TransferOwnershipIndex {
    owners: DashMap<u32, SubscriptionOwnershipState>,
}

impl TransferOwnershipIndex {
    pub(crate) fn register(&self, subscription_id: u32, owner: SubscriptionOwnershipState) {
        self.owners.insert(subscription_id, owner);
    }

    pub(crate) fn attach(&self, subscription_id: u32, owner_session_id: NodeId) {
        self.owners.insert(
            subscription_id,
            SubscriptionOwnershipState {
                owner_session_id: Some(owner_session_id),
                detached_restored: false,
            },
        );
    }

    pub(crate) fn register_detached_restore(
        &self,
        subscription_id: u32,
        previous_owner_session_id: Option<NodeId>,
    ) {
        self.owners.insert(
            subscription_id,
            SubscriptionOwnershipState {
                owner_session_id: previous_owner_session_id,
                detached_restored: true,
            },
        );
    }

    pub(crate) fn remove(&self, subscription_id: u32) -> Option<SubscriptionOwnershipState> {
        self.owners.remove(&subscription_id).map(|(_, state)| state)
    }

    pub(crate) fn state(&self, subscription_id: u32) -> Option<SubscriptionOwnershipState> {
        self.owners.get(&subscription_id).map(|entry| entry.clone())
    }

    pub(crate) fn detached_count(&self) -> usize {
        self.owners
            .iter()
            .filter(|entry| entry.detached_restored)
            .count()
    }
}

#[derive(Debug, Default)]
pub(crate) struct MonitoredItemCatalog {
    items: HashMap<u32, MonitoredItem>,
}

impl MonitoredItemCatalog {
    pub(crate) fn create(
        &mut self,
        state: &mut SubscriptionStateData,
        config: MonitoredItemConfig,
    ) -> u32 {
        let item_id = state.next_item_id;
        state.next_item_id += 1;

        let item = MonitoredItem::new(item_id, config);
        self.items.insert(item_id, item);
        item_id
    }

    pub(crate) fn delete(&mut self, item_id: u32) -> bool {
        self.items.remove(&item_id).is_some()
    }

    pub(crate) fn get(&self, item_id: u32) -> Option<&MonitoredItem> {
        self.items.get(&item_id)
    }

    pub(crate) fn get_mut(&mut self, item_id: u32) -> Option<&mut MonitoredItem> {
        self.items.get_mut(&item_id)
    }

    pub(crate) fn ids(&self) -> Vec<u32> {
        self.items.keys().copied().collect()
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn on_value_change(
        &mut self,
        node_id: &NodeId,
        value: DataValue,
        queue: &mut NotificationQueue,
    ) {
        for item in self.items.values_mut() {
            if &item.config().node_id == node_id {
                if let Some(notification) = item.on_value_change(value.clone()) {
                    queue.push_data_change(notification);
                }
            }
        }
    }

    pub(crate) fn event_monitored_items(&self) -> Vec<(u32, EventFilter)> {
        self.items
            .values()
            .filter_map(|item| {
                if item.config().kind == MonitoredItemKind::Event {
                    item.config()
                        .event_filter
                        .as_ref()
                        .map(|filter| (item.client_handle(), filter.clone()))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SubscriptionScheduleBucket {
    pub(crate) interval_ms: u64,
    pub(crate) subscription_ids: Vec<u32>,
}

#[derive(Debug, Default)]
pub(crate) struct SubscriptionScheduler;

impl SubscriptionScheduler {
    pub(crate) fn schedule(
        &self,
        catalog: &SubscriptionCatalog,
    ) -> Vec<SubscriptionScheduleBucket> {
        let mut buckets = BTreeMap::<u64, Vec<u32>>::new();
        for entry in catalog.subscriptions.iter() {
            let subscription = entry.value().read();
            let bucket = subscription.sampling_bucket_ms();
            buckets.entry(bucket).or_default().push(*entry.key());
        }
        buckets
            .into_iter()
            .map(
                |(interval_ms, subscription_ids)| SubscriptionScheduleBucket {
                    interval_ms,
                    subscription_ids,
                },
            )
            .collect()
    }
}

#[derive(Debug, Default)]
pub(crate) struct NotificationQueue {
    pending_notifications: Vec<MonitoredItemNotification>,
    pending_event_notifications: Vec<EventFieldList>,
}

impl NotificationQueue {
    pub(crate) fn snapshot(&self) -> (Vec<MonitoredItemNotification>, Vec<EventFieldList>) {
        (
            self.pending_notifications.clone(),
            self.pending_event_notifications.clone(),
        )
    }

    pub(crate) fn from_snapshot(
        pending_notifications: Vec<MonitoredItemNotification>,
        pending_event_notifications: Vec<EventFieldList>,
    ) -> Self {
        Self {
            pending_notifications,
            pending_event_notifications,
        }
    }
}

impl NotificationQueue {
    pub(crate) fn push_data_change(&mut self, notification: MonitoredItemNotification) {
        self.pending_notifications.push(notification);
    }

    pub(crate) fn push_event_notification(
        &mut self,
        max_notifications: u32,
        field_list: EventFieldList,
    ) {
        let max_queue = max_notifications as usize * 2;
        if self.pending_event_notifications.len() < max_queue {
            self.pending_event_notifications.push(field_list);
        }
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.pending_notifications.is_empty() || !self.pending_event_notifications.is_empty()
    }
}

pub(crate) struct PublishEngine;

impl PublishEngine {
    pub(crate) fn process_publish(subscription: &mut Subscription) -> Option<NotificationMessage> {
        if !subscription.is_publishable() {
            return None;
        }

        if !subscription.state_data.config.publishing_enabled {
            return None;
        }

        subscription.state_data.reset_lifetime();

        let has_data_notifications = !subscription
            .notification_queue
            .pending_notifications
            .is_empty();
        let has_event_notifications = !subscription
            .notification_queue
            .pending_event_notifications
            .is_empty();

        if !has_data_notifications && !has_event_notifications {
            subscription.state_data.keep_alive_counter += 1;

            if subscription.state_data.keep_alive_counter
                >= subscription.state_data.config.max_keep_alive_count
            {
                subscription.state_data.mark_keepalive();
                return Some(NotificationMessage {
                    subscription_id: subscription.state_data.config.subscription_id,
                    sequence_number: subscription.state_data.sequence_number,
                    publish_time: Utc::now(),
                    notifications: Vec::new(),
                    event_notifications: Vec::new(),
                    more_notifications: false,
                });
            }

            return None;
        }

        let max = subscription.state_data.config.max_notifications_per_publish as usize;
        let notifications = if subscription.notification_queue.pending_notifications.len() > max {
            subscription
                .notification_queue
                .pending_notifications
                .drain(..max)
                .collect()
        } else {
            std::mem::take(&mut subscription.notification_queue.pending_notifications)
        };

        let event_budget = max.saturating_sub(notifications.len());
        let event_notifications = if event_budget == 0 {
            Vec::new()
        } else if subscription
            .notification_queue
            .pending_event_notifications
            .len()
            > event_budget
        {
            subscription
                .notification_queue
                .pending_event_notifications
                .drain(..event_budget)
                .collect()
        } else {
            std::mem::take(&mut subscription.notification_queue.pending_event_notifications)
        };

        let more_notifications = subscription.notification_queue.has_pending();
        let message = NotificationMessage {
            subscription_id: subscription.state_data.config.subscription_id,
            sequence_number: subscription.state_data.sequence_number,
            publish_time: Utc::now(),
            notifications,
            event_notifications,
            more_notifications,
        };

        subscription.state_data.mark_published();
        Some(message)
    }

    pub(crate) fn build_publish_response(
        subscription_id: u32,
        subscription: &mut Subscription,
    ) -> PublishResponse {
        let notification_message = Self::process_publish(subscription);

        PublishResponse {
            subscription_id,
            available_sequence_numbers: vec![subscription.state_data.sequence_number],
            more_notifications: subscription.notification_queue.has_pending(),
            notification_message,
        }
    }
}

pub(crate) struct SubscriptionCatalog {
    subscriptions: DashMap<u32, RwLock<Subscription>>,
    ownership: TransferOwnershipIndex,
    next_subscription_id: AtomicU32,
}

impl SubscriptionCatalog {
    pub(crate) fn new() -> Self {
        Self {
            subscriptions: DashMap::new(),
            ownership: TransferOwnershipIndex::default(),
            next_subscription_id: AtomicU32::new(1),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.subscriptions.len()
    }

    pub(crate) fn get(
        &self,
        subscription_id: u32,
    ) -> Option<dashmap::mapref::one::Ref<'_, u32, RwLock<Subscription>>> {
        self.subscriptions.get(&subscription_id)
    }

    pub(crate) fn create_subscription(
        &self,
        max_subscriptions: usize,
        config: SubscriptionConfig,
    ) -> Result<u32, SubscriptionError> {
        self.create_subscription_for_owner(max_subscriptions, config, None)
    }

    pub(crate) fn create_subscription_for_owner(
        &self,
        max_subscriptions: usize,
        mut config: SubscriptionConfig,
        owner_session_id: Option<NodeId>,
    ) -> Result<u32, SubscriptionError> {
        if self.subscriptions.len() >= max_subscriptions {
            return Err(SubscriptionError::MaxSubscriptionsReached);
        }

        let id = self.next_subscription_id.fetch_add(1, Ordering::SeqCst);
        config.subscription_id = id;
        let ownership = SubscriptionOwnershipState {
            owner_session_id: owner_session_id.clone(),
            detached_restored: false,
        };
        self.subscriptions.insert(
            id,
            RwLock::new(Subscription::new_with_ownership(config, ownership.clone())),
        );
        self.ownership.register(id, ownership);
        Ok(id)
    }

    pub(crate) fn remove(&self, subscription_id: u32) -> bool {
        self.remove_with_ownership(subscription_id).is_some()
    }

    pub(crate) fn remove_with_ownership(
        &self,
        subscription_id: u32,
    ) -> Option<SubscriptionOwnershipState> {
        let removed = self.subscriptions.remove(&subscription_id);
        let ownership = self.ownership.remove(subscription_id);
        removed.map(|_| ownership.unwrap_or_default())
    }

    pub(crate) fn ids(&self) -> Vec<u32> {
        self.subscriptions
            .iter()
            .map(|entry| *entry.key())
            .collect()
    }

    pub(crate) fn for_each_mut<F>(&self, mut apply: F)
    where
        F: FnMut(&mut Subscription),
    {
        for entry in self.subscriptions.iter() {
            let mut subscription = entry.value().write();
            apply(&mut subscription);
        }
    }

    pub(crate) fn replace_from_snapshot(&self, snapshot: DurableSubscriptionSnapshot) {
        self.subscriptions.clear();
        self.ownership.owners.clear();
        let mut max_id = 1;
        for record in snapshot.subscriptions {
            let subscription_id = record.subscription_id;
            max_id = max_id.max(record.subscription_id.saturating_add(1));
            let previous_owner = record.ownership.owner_session_id.clone();
            let subscription = Subscription::from_persisted(record);
            self.subscriptions
                .insert(subscription.id(), RwLock::new(subscription));
            self.ownership
                .register_detached_restore(subscription_id, previous_owner);
        }
        self.next_subscription_id.store(max_id, Ordering::SeqCst);
    }

    pub(crate) fn snapshot(
        &self,
        metadata: DurableSubscriptionMetadata,
    ) -> DurableSubscriptionSnapshot {
        DurableSubscriptionSnapshot {
            metadata,
            subscriptions: self
                .subscriptions
                .iter()
                .map(|entry| entry.value().read().snapshot())
                .collect(),
        }
    }

    pub(crate) fn ownership_state(
        &self,
        subscription_id: u32,
    ) -> Option<SubscriptionOwnershipState> {
        self.ownership.state(subscription_id)
    }

    pub(crate) fn detached_subscription_count(&self) -> usize {
        self.ownership.detached_count()
    }

    pub(crate) fn transfer_subscription(
        &self,
        subscription_id: u32,
        owner_session_id: NodeId,
    ) -> Result<SubscriptionOwnershipState, SubscriptionError> {
        let subscription = self
            .subscriptions
            .get(&subscription_id)
            .ok_or(SubscriptionError::SubscriptionNotFound)?;
        let mut subscription = subscription.write();
        let previous = subscription.ownership_state();
        subscription.attach_owner(owner_session_id.clone());
        self.ownership.attach(subscription_id, owner_session_id);
        Ok(previous)
    }
}

pub(crate) struct SubscriptionRuntime {
    scheduler: SubscriptionScheduler,
}

impl SubscriptionRuntime {
    pub(crate) fn new() -> Self {
        Self {
            scheduler: SubscriptionScheduler,
        }
    }

    pub(crate) async fn tick_all(
        &self,
        catalog: &SubscriptionCatalog,
        notification_tx: &mpsc::Sender<NotificationMessage>,
        event_tx: &broadcast::Sender<SubscriptionEvent>,
    ) -> bool {
        let mut changed = false;
        let mut messages_to_send = Vec::new();
        for bucket in self.scheduler.schedule(catalog) {
            let _interval_ms = bucket.interval_ms;
            for subscription_id in bucket.subscription_ids {
                let Some(entry) = catalog.subscriptions.get(&subscription_id) else {
                    continue;
                };
                let mut subscription = entry.value().write();
                subscription.tick();
                let published =
                    PublishEngine::process_publish(&mut subscription).filter(|message| {
                        !message.notifications.is_empty() || !message.event_notifications.is_empty()
                    });
                if published.is_some() {
                    changed = true;
                }
                if let Some(message) = published {
                    messages_to_send.push(message);
                }
            }
        }

        for message in messages_to_send {
            let _ = notification_tx.send(message.clone()).await;
            let _ = event_tx.send(SubscriptionEvent::Notification(message));
        }

        let to_delete: Vec<u32> = catalog
            .subscriptions
            .iter()
            .filter(|entry| entry.value().read().should_delete())
            .map(|entry| *entry.key())
            .collect();

        for id in to_delete {
            catalog.remove(id);
            let _ = event_tx.send(SubscriptionEvent::Deleted {
                subscription_id: id,
            });
            changed = true;
        }
        changed
    }
}

impl Subscription {
    pub(crate) fn snapshot(&self) -> PersistedSubscriptionRecord {
        let (pending_notifications, pending_event_notifications) =
            self.notification_queue.snapshot();
        PersistedSubscriptionRecord {
            subscription_id: self.id(),
            state_data: self.state_data.clone(),
            monitored_items: self
                .monitored_items
                .items
                .values()
                .map(MonitoredItem::snapshot)
                .collect(),
            pending_notifications,
            pending_event_notifications,
            ownership: PersistedSubscriptionOwnership {
                owner_session_id: self.owner_session_id.clone(),
                detached_restored: self.detached_restored,
            },
        }
    }

    pub(crate) fn from_persisted(record: PersistedSubscriptionRecord) -> Self {
        let mut items = HashMap::new();
        for item in record.monitored_items {
            items.insert(item.id, MonitoredItem::from_persisted(item));
        }
        Self {
            state_data: record.state_data,
            monitored_items: MonitoredItemCatalog { items },
            notification_queue: NotificationQueue::from_snapshot(
                record.pending_notifications,
                record.pending_event_notifications,
            ),
            owner_session_id: None,
            detached_restored: true,
            created_at: Utc::now(),
        }
    }
}
