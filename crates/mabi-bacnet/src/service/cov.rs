//! COV (Change of Value) subscription and notification services.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tokio::sync::mpsc;

use crate::apdu::encoding::ApduEncoder;
use crate::object::property::{BACnetValue, PropertyId};
use crate::object::types::ObjectId;

/// COV subscription.
#[derive(Debug, Clone)]
pub struct CovSubscription {
    /// Subscriber address.
    pub subscriber_address: SocketAddr,
    /// Subscriber process identifier.
    pub subscriber_process_id: u32,
    /// Monitored object identifier.
    pub monitored_object: ObjectId,
    /// Whether to send confirmed notifications.
    pub confirmed_notifications: bool,
    /// Subscription lifetime (None = infinite).
    pub lifetime: Option<Duration>,
    /// COV increment for analog objects.
    pub cov_increment: Option<f32>,
    /// When the subscription was created.
    pub created_at: Instant,
    /// Last notification time.
    pub last_notification: Option<Instant>,
}

impl CovSubscription {
    /// Create a new subscription.
    pub fn new(
        subscriber_address: SocketAddr,
        subscriber_process_id: u32,
        monitored_object: ObjectId,
        confirmed: bool,
        lifetime: Option<Duration>,
    ) -> Self {
        Self {
            subscriber_address,
            subscriber_process_id,
            monitored_object,
            confirmed_notifications: confirmed,
            lifetime,
            cov_increment: None,
            created_at: Instant::now(),
            last_notification: None,
        }
    }

    /// Check if the subscription has expired.
    pub fn is_expired(&self) -> bool {
        if let Some(lifetime) = self.lifetime {
            self.created_at.elapsed() > lifetime
        } else {
            false
        }
    }

    /// Get remaining lifetime in seconds.
    pub fn remaining_lifetime(&self) -> Option<u32> {
        self.lifetime.map(|lifetime| {
            let elapsed = self.created_at.elapsed();
            if elapsed > lifetime {
                0
            } else {
                (lifetime - elapsed).as_secs() as u32
            }
        })
    }
}

/// Key for subscription lookup.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SubscriptionKey {
    subscriber_address: SocketAddr,
    subscriber_process_id: u32,
    monitored_object: ObjectId,
}

/// COV notification to be sent.
#[derive(Debug, Clone)]
pub struct CovNotification {
    /// Destination address.
    pub destination: SocketAddr,
    /// Subscriber process ID.
    pub subscriber_process_id: u32,
    /// Initiating device identifier.
    pub initiating_device: ObjectId,
    /// Monitored object identifier.
    pub monitored_object: ObjectId,
    /// Time remaining on subscription.
    pub time_remaining: u32,
    /// List of property values.
    pub list_of_values: Vec<(PropertyId, BACnetValue)>,
    /// Whether this requires confirmation.
    pub confirmed: bool,
}

impl CovNotification {
    /// Encode as unconfirmed COV notification APDU.
    pub fn encode_unconfirmed(&self) -> Vec<u8> {
        let mut encoder = ApduEncoder::new();

        // Context tag 0: Subscriber Process Identifier
        encoder.encode_context_unsigned(0, self.subscriber_process_id);

        // Context tag 1: Initiating Device Identifier
        encoder.encode_context_object_identifier(1, self.initiating_device);

        // Context tag 2: Monitored Object Identifier
        encoder.encode_context_object_identifier(2, self.monitored_object);

        // Context tag 3: Time Remaining
        encoder.encode_context_unsigned(3, self.time_remaining);

        // Context tag 4: List of Values (opening)
        encoder.encode_opening_tag(4);
        for (property_id, value) in &self.list_of_values {
            // Property Identifier
            encoder.encode_context_enumerated(0, *property_id as u32);
            // Property Value (opening tag 2)
            encoder.encode_opening_tag(2);
            encoder.encode_value(value);
            encoder.encode_closing_tag(2);
        }
        encoder.encode_closing_tag(4);

        encoder.into_bytes()
    }
}

/// COV subscription manager.
pub struct CovManager {
    /// Active subscriptions.
    subscriptions: DashMap<SubscriptionKey, CovSubscription>,
    /// Channel for outgoing notifications.
    notification_tx: mpsc::Sender<CovNotification>,
    /// Maximum number of subscriptions.
    max_subscriptions: usize,
    /// Device instance for notifications.
    device_instance: u32,
}

impl CovManager {
    /// Create a new COV manager.
    pub fn new(
        device_instance: u32,
        max_subscriptions: usize,
    ) -> (Self, mpsc::Receiver<CovNotification>) {
        let (tx, rx) = mpsc::channel(1000);

        (
            Self {
                subscriptions: DashMap::new(),
                notification_tx: tx,
                max_subscriptions,
                device_instance,
            },
            rx,
        )
    }

    /// Add or update a subscription.
    pub fn subscribe(&self, subscription: CovSubscription) -> Result<(), CovError> {
        // Check if we're at capacity
        if self.subscriptions.len() >= self.max_subscriptions {
            // Try to remove expired subscriptions first
            self.cleanup_expired();

            if self.subscriptions.len() >= self.max_subscriptions {
                return Err(CovError::MaxSubscriptionsReached);
            }
        }

        let key = SubscriptionKey {
            subscriber_address: subscription.subscriber_address,
            subscriber_process_id: subscription.subscriber_process_id,
            monitored_object: subscription.monitored_object,
        };

        self.subscriptions.insert(key, subscription);
        Ok(())
    }

    /// Cancel a subscription.
    pub fn unsubscribe(
        &self,
        subscriber_address: SocketAddr,
        subscriber_process_id: u32,
        monitored_object: ObjectId,
    ) -> bool {
        let key = SubscriptionKey {
            subscriber_address,
            subscriber_process_id,
            monitored_object,
        };

        self.subscriptions.remove(&key).is_some()
    }

    /// Get subscription count.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Get subscriptions for an object.
    pub fn subscriptions_for_object(&self, object_id: ObjectId) -> Vec<CovSubscription> {
        self.subscriptions
            .iter()
            .filter(|entry| entry.monitored_object == object_id)
            .map(|entry| entry.clone())
            .collect()
    }

    /// Notify value change for an object (called when object changes).
    pub async fn notify_change(
        &self,
        object_id: ObjectId,
        values: Vec<(PropertyId, BACnetValue)>,
    ) {
        let device_id = ObjectId::device(self.device_instance);

        for entry in self.subscriptions.iter() {
            let subscription = entry.value();

            if subscription.monitored_object != object_id || subscription.is_expired() {
                continue;
            }

            let notification = CovNotification {
                destination: subscription.subscriber_address,
                subscriber_process_id: subscription.subscriber_process_id,
                initiating_device: device_id,
                monitored_object: object_id,
                time_remaining: subscription.remaining_lifetime().unwrap_or(0),
                list_of_values: values.clone(),
                confirmed: subscription.confirmed_notifications,
            };

            let _ = self.notification_tx.send(notification).await;
        }
    }

    /// Clean up expired subscriptions.
    pub fn cleanup_expired(&self) {
        self.subscriptions.retain(|_, sub| !sub.is_expired());
    }

    /// Get all active subscriptions as a list.
    pub fn list_subscriptions(&self) -> Vec<CovSubscription> {
        self.subscriptions.iter().map(|entry| entry.clone()).collect()
    }
}

/// COV errors.
#[derive(Debug, thiserror::Error)]
pub enum CovError {
    #[error("Maximum subscriptions reached")]
    MaxSubscriptionsReached,

    #[error("Subscription not found")]
    SubscriptionNotFound,

    #[error("Object does not support COV")]
    NotCovProperty,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscription_expiry() {
        let sub = CovSubscription::new(
            "127.0.0.1:47808".parse().unwrap(),
            1,
            ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1),
            false,
            Some(Duration::from_secs(0)), // Expired immediately
        );

        std::thread::sleep(Duration::from_millis(10));
        assert!(sub.is_expired());
    }

    #[test]
    fn test_subscription_no_expiry() {
        let sub = CovSubscription::new(
            "127.0.0.1:47808".parse().unwrap(),
            1,
            ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1),
            false,
            None, // No expiry
        );

        assert!(!sub.is_expired());
    }

    #[tokio::test]
    async fn test_cov_manager() {
        let (manager, _rx) = CovManager::new(1234, 100);

        let sub = CovSubscription::new(
            "127.0.0.1:47808".parse().unwrap(),
            1,
            ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1),
            false,
            Some(Duration::from_secs(300)),
        );

        manager.subscribe(sub).unwrap();
        assert_eq!(manager.subscription_count(), 1);

        manager.unsubscribe(
            "127.0.0.1:47808".parse().unwrap(),
            1,
            ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1),
        );
        assert_eq!(manager.subscription_count(), 0);
    }
}
