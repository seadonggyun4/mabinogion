//! OPC UA services implementation.
//!
//! This module provides the core OPC UA services:
//! - Subscription management
//! - Monitored items
//! - Historical data access
//! - Session management

pub mod history;
pub mod monitored_item;
pub mod session;
pub mod subscription;

pub use history::{HistoryStore, HistoryStoreConfig, HistoricalDataPoint, AggregateType};
pub use monitored_item::{
    MonitoredItem, MonitoredItemConfig, MonitoredItemNotification, MonitoringMode,
    DataChangeFilter, DataChangeTrigger, DeadbandType,
};
pub use session::{SessionManager, SessionManagerConfig, Session, SessionInfo, SessionEvent, UserIdentity};
pub use subscription::{
    Subscription, SubscriptionConfig, SubscriptionManager, SubscriptionManagerConfig,
    SubscriptionEvent, PublishResponse, NotificationMessage,
};
