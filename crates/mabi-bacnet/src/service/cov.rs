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
    /// Encode the COV notification service data (shared between confirmed and unconfirmed).
    fn encode_service_data(&self) -> Vec<u8> {
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

    /// Encode as unconfirmed COV notification APDU service data.
    pub fn encode_unconfirmed(&self) -> Vec<u8> {
        self.encode_service_data()
    }

    /// Encode as confirmed COV notification APDU.
    ///
    /// Returns the full APDU including the confirmed request header.
    /// `invoke_id` is the invoke ID for the confirmed transaction.
    pub fn encode_confirmed(&self, invoke_id: u8) -> Vec<u8> {
        let service_data = self.encode_service_data();
        let mut apdu = Vec::with_capacity(4 + service_data.len());
        // Confirmed Request header: PDU type 0 (ConfirmedRequest), no segmentation
        apdu.push(0x00); // PDU type 0, no segmentation, no more-follows
        apdu.push(0x05); // max-segments=0, max-apdu=5 (1476 bytes)
        apdu.push(invoke_id);
        apdu.push(crate::apdu::types::ConfirmedService::ConfirmedCovNotification as u8);
        apdu.extend_from_slice(&service_data);
        apdu
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
    pub async fn notify_change(&self, object_id: ObjectId, values: Vec<(PropertyId, BACnetValue)>) {
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
        self.subscriptions
            .iter()
            .map(|entry| entry.clone())
            .collect()
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

    #[test]
    fn test_subscription_remaining_lifetime() {
        let sub = CovSubscription::new(
            "127.0.0.1:47808".parse().unwrap(),
            1,
            ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1),
            false,
            Some(Duration::from_secs(300)),
        );

        let remaining = sub.remaining_lifetime().unwrap();
        assert!(remaining > 0 && remaining <= 300);

        // Infinite lifetime returns None
        let sub_inf = CovSubscription::new(
            "127.0.0.1:47808".parse().unwrap(),
            1,
            ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1),
            false,
            None,
        );
        assert!(sub_inf.remaining_lifetime().is_none());
    }

    #[test]
    fn test_subscription_cov_increment() {
        let mut sub = CovSubscription::new(
            "127.0.0.1:47808".parse().unwrap(),
            1,
            ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1),
            false,
            None,
        );

        assert!(sub.cov_increment.is_none());
        sub.cov_increment = Some(1.5);
        assert_eq!(sub.cov_increment, Some(1.5));
    }

    #[test]
    fn test_encode_unconfirmed_notification() {
        let notification = CovNotification {
            destination: "10.0.0.1:47808".parse().unwrap(),
            subscriber_process_id: 42,
            initiating_device: ObjectId::device(1234),
            monitored_object: ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1),
            time_remaining: 300,
            list_of_values: vec![
                (PropertyId::PresentValue, BACnetValue::Real(72.5)),
                (
                    PropertyId::StatusFlags,
                    BACnetValue::BitString(vec![false, false, false, false]),
                ),
            ],
            confirmed: false,
        };

        let data = notification.encode_unconfirmed();
        assert!(!data.is_empty());
        // Should start with context tag 0 (subscriber process id)
        assert_eq!(data[0] & 0x0F, 0x09); // context tag 0, length 1
    }

    #[test]
    fn test_encode_confirmed_notification() {
        let notification = CovNotification {
            destination: "10.0.0.1:47808".parse().unwrap(),
            subscriber_process_id: 1,
            initiating_device: ObjectId::device(1234),
            monitored_object: ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1),
            time_remaining: 0,
            list_of_values: vec![(PropertyId::PresentValue, BACnetValue::Real(25.0))],
            confirmed: true,
        };

        let apdu = notification.encode_confirmed(7);
        // ConfirmedRequest header
        assert_eq!(apdu[0], 0x00); // PDU type 0, no segmentation
        assert_eq!(apdu[1], 0x05); // max-segments=0, max-apdu=5 (1476)
        assert_eq!(apdu[2], 7); // invoke_id
        assert_eq!(
            apdu[3],
            crate::apdu::types::ConfirmedService::ConfirmedCovNotification as u8
        );
        // Service data follows
        assert!(apdu.len() > 4);
    }

    #[tokio::test]
    async fn test_cov_manager_max_subscriptions() {
        let (manager, _rx) = CovManager::new(1234, 2);

        let sub1 = CovSubscription::new(
            "10.0.0.1:47808".parse().unwrap(),
            1,
            ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1),
            false,
            None,
        );
        let sub2 = CovSubscription::new(
            "10.0.0.2:47808".parse().unwrap(),
            1,
            ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1),
            false,
            None,
        );
        let sub3 = CovSubscription::new(
            "10.0.0.3:47808".parse().unwrap(),
            1,
            ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1),
            false,
            None,
        );

        manager.subscribe(sub1).unwrap();
        manager.subscribe(sub2).unwrap();
        assert!(manager.subscribe(sub3).is_err());
    }

    #[tokio::test]
    async fn test_cov_manager_cleanup_expired() {
        let (manager, _rx) = CovManager::new(1234, 100);

        let sub = CovSubscription::new(
            "127.0.0.1:47808".parse().unwrap(),
            1,
            ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1),
            false,
            Some(Duration::from_secs(0)), // Expires immediately
        );

        manager.subscribe(sub).unwrap();
        assert_eq!(manager.subscription_count(), 1);

        std::thread::sleep(Duration::from_millis(10));
        manager.cleanup_expired();
        assert_eq!(manager.subscription_count(), 0);
    }

    #[tokio::test]
    async fn test_cov_manager_subscriptions_for_object() {
        let (manager, _rx) = CovManager::new(1234, 100);
        let obj1 = ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1);
        let obj2 = ObjectId::new(crate::object::types::ObjectType::AnalogInput, 2);

        manager
            .subscribe(CovSubscription::new(
                "10.0.0.1:47808".parse().unwrap(),
                1,
                obj1,
                false,
                None,
            ))
            .unwrap();
        manager
            .subscribe(CovSubscription::new(
                "10.0.0.2:47808".parse().unwrap(),
                1,
                obj1,
                true,
                None,
            ))
            .unwrap();
        manager
            .subscribe(CovSubscription::new(
                "10.0.0.3:47808".parse().unwrap(),
                1,
                obj2,
                false,
                None,
            ))
            .unwrap();

        let subs = manager.subscriptions_for_object(obj1);
        assert_eq!(subs.len(), 2);

        let subs2 = manager.subscriptions_for_object(obj2);
        assert_eq!(subs2.len(), 1);
    }

    #[tokio::test]
    async fn test_cov_notify_change() {
        let (manager, mut rx) = CovManager::new(1234, 100);
        let obj = ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1);

        // Subscribe two subscribers, one confirmed, one unconfirmed
        manager
            .subscribe(CovSubscription::new(
                "10.0.0.1:47808".parse().unwrap(),
                1,
                obj,
                false,
                None,
            ))
            .unwrap();
        manager
            .subscribe(CovSubscription::new(
                "10.0.0.2:47808".parse().unwrap(),
                2,
                obj,
                true,
                None,
            ))
            .unwrap();

        // Trigger notification
        let values = vec![(PropertyId::PresentValue, BACnetValue::Real(42.0))];
        manager.notify_change(obj, values).await;

        // Should receive 2 notifications
        let n1 = rx.try_recv().unwrap();
        let n2 = rx.try_recv().unwrap();

        // One confirmed, one not
        let (confirmed_count, unconfirmed_count) =
            [n1, n2].iter().fold(
                (0, 0),
                |(c, u), n| {
                    if n.confirmed {
                        (c + 1, u)
                    } else {
                        (c, u + 1)
                    }
                },
            );
        assert_eq!(confirmed_count, 1);
        assert_eq!(unconfirmed_count, 1);
    }

    #[tokio::test]
    async fn test_cov_manager_list_subscriptions() {
        let (manager, _rx) = CovManager::new(1234, 100);
        let obj = ObjectId::new(crate::object::types::ObjectType::AnalogInput, 1);

        manager
            .subscribe(CovSubscription::new(
                "10.0.0.1:47808".parse().unwrap(),
                1,
                obj,
                false,
                None,
            ))
            .unwrap();
        manager
            .subscribe(CovSubscription::new(
                "10.0.0.2:47808".parse().unwrap(),
                2,
                obj,
                true,
                Some(Duration::from_secs(600)),
            ))
            .unwrap();

        let list = manager.list_subscriptions();
        assert_eq!(list.len(), 2);
    }
}
