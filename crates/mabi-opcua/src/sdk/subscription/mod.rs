use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::sync::{broadcast, mpsc};

use crate::types::{DataValue, NodeId};

pub(crate) mod event {
    pub(crate) use crate::sdk::event::{EventFieldList, EventFilter};
}

#[path = "../../services/monitored_item.rs"]
pub(crate) mod monitored_item;
#[path = "../../services/subscription.rs"]
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

pub(crate) trait EventSubscriptionPort: Send + Sync {
    fn subscription_ids(&self) -> Vec<u32>;
    fn get_event_monitored_items(&self, subscription_id: u32) -> Vec<(u32, EventFilter)>;
    fn push_event_notification(&self, subscription_id: u32, field_list: EventFieldList);
}

#[derive(Debug)]
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

#[derive(Debug, Default)]
pub(crate) struct NotificationQueue {
    pending_notifications: Vec<MonitoredItemNotification>,
    pending_event_notifications: Vec<EventFieldList>,
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
    next_subscription_id: AtomicU32,
}

impl SubscriptionCatalog {
    pub(crate) fn new() -> Self {
        Self {
            subscriptions: DashMap::new(),
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
        mut config: SubscriptionConfig,
    ) -> Result<u32, SubscriptionError> {
        if self.subscriptions.len() >= max_subscriptions {
            return Err(SubscriptionError::MaxSubscriptionsReached);
        }

        let id = self.next_subscription_id.fetch_add(1, Ordering::SeqCst);
        config.subscription_id = id;
        self.subscriptions
            .insert(id, RwLock::new(Subscription::new(config)));
        Ok(id)
    }

    pub(crate) fn remove(&self, subscription_id: u32) -> bool {
        self.subscriptions.remove(&subscription_id).is_some()
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
}

pub(crate) struct SubscriptionRuntime;

impl SubscriptionRuntime {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) async fn tick_all(
        &self,
        catalog: &SubscriptionCatalog,
        notification_tx: &mpsc::Sender<NotificationMessage>,
        event_tx: &broadcast::Sender<SubscriptionEvent>,
    ) {
        let messages_to_send: Vec<NotificationMessage> = catalog
            .subscriptions
            .iter()
            .filter_map(|entry| {
                let mut subscription = entry.value().write();
                subscription.tick();
                PublishEngine::process_publish(&mut subscription).filter(|message| {
                    !message.notifications.is_empty() || !message.event_notifications.is_empty()
                })
            })
            .collect();

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
        }
    }
}
