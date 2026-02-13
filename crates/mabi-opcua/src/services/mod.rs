//! OPC UA services implementation.
//!
//! This module provides the core OPC UA services:
//! - Subscription management
//! - Monitored items
//! - Historical data access
//! - Session management
//! - Event management

pub mod event;
pub mod history;
pub mod monitored_item;
pub mod session;
pub mod subscription;

pub use event::{
    EventManager, EventData, EventFilter, EventFieldList, EventNotification,
    SimpleAttributeOperand, ContentFilterElement, FilterOperator, FilterOperand,
};
pub use history::{HistoryStore, HistoryStoreConfig, HistoricalDataPoint, AggregateType};
pub use monitored_item::{
    MonitoredItem, MonitoredItemConfig, MonitoredItemNotification, MonitoringMode,
    MonitoredItemKind, DataChangeFilter, DataChangeTrigger, DeadbandType,
};
pub use session::{SessionManager, SessionManagerConfig, Session, SessionInfo, SessionEvent, UserIdentity};
pub use subscription::{
    Subscription, SubscriptionConfig, SubscriptionManager, SubscriptionManagerConfig,
    SubscriptionEvent, PublishResponse, NotificationMessage,
};
