//! KNX Group Object management.
//!
//! This module provides the group object table for managing KNX group addresses
//! and their associated values.

use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::sync::broadcast;

use crate::address::GroupAddress;
use crate::dpt::{BoxedDptCodec, DptId, DptRegistry, DptValue};
use crate::error::{KnxError, KnxResult};

// ============================================================================
// Group Object Flags
// ============================================================================

/// Group object communication flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct GroupObjectFlags {
    /// Communication enabled.
    pub communication: bool,
    /// Read enabled.
    pub read: bool,
    /// Write enabled.
    pub write: bool,
    /// Transmit on change.
    pub transmit: bool,
    /// Update on receive.
    pub update: bool,
}

impl GroupObjectFlags {
    /// Full read-write access.
    pub fn read_write() -> Self {
        Self {
            communication: true,
            read: true,
            write: true,
            transmit: true,
            update: true,
        }
    }

    /// Read-only access.
    pub fn read_only() -> Self {
        Self {
            communication: true,
            read: true,
            write: false,
            transmit: true,
            update: false,
        }
    }

    /// Write-only access.
    pub fn write_only() -> Self {
        Self {
            communication: true,
            read: false,
            write: true,
            transmit: false,
            update: true,
        }
    }

    /// Check if any communication is enabled.
    pub fn is_enabled(&self) -> bool {
        self.communication
    }
}

// ============================================================================
// Group Object
// ============================================================================

/// A KNX group object.
pub struct GroupObject {
    /// Group address.
    address: GroupAddress,
    /// Object name.
    name: String,
    /// Description.
    description: String,
    /// DPT codec.
    codec: Arc<BoxedDptCodec>,
    /// Communication flags.
    flags: GroupObjectFlags,
    /// Current raw value.
    value: RwLock<Vec<u8>>,
    /// Last update timestamp.
    last_update: RwLock<std::time::Instant>,
}

impl std::fmt::Debug for GroupObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GroupObject")
            .field("address", &self.address)
            .field("name", &self.name)
            .field("description", &self.description)
            .field("dpt_id", &self.codec.id())
            .field("flags", &self.flags)
            .field("value", &self.value.read().as_slice())
            .finish()
    }
}

impl GroupObject {
    /// Create a new group object.
    pub fn new(address: GroupAddress, name: impl Into<String>, codec: Arc<BoxedDptCodec>) -> Self {
        let default_value = codec.default_value();
        let encoded = codec.encode(&default_value).unwrap_or_default();

        Self {
            address,
            name: name.into(),
            description: String::new(),
            codec,
            flags: GroupObjectFlags::read_write(),
            value: RwLock::new(encoded),
            last_update: RwLock::new(std::time::Instant::now()),
        }
    }

    /// Create with description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set flags.
    pub fn with_flags(mut self, flags: GroupObjectFlags) -> Self {
        self.flags = flags;
        self
    }

    /// Set initial value.
    pub fn with_value(self, value: &DptValue) -> KnxResult<Self> {
        self.write_value(value)?;
        Ok(self)
    }

    /// Get group address.
    pub fn address(&self) -> GroupAddress {
        self.address
    }

    /// Get name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get description.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Get DPT ID.
    pub fn dpt_id(&self) -> DptId {
        self.codec.id()
    }

    /// Get flags.
    pub fn flags(&self) -> GroupObjectFlags {
        self.flags
    }

    /// Get raw value.
    pub fn read_raw(&self) -> Vec<u8> {
        self.value.read().clone()
    }

    /// Read decoded value.
    pub fn read_value(&self) -> KnxResult<DptValue> {
        let raw = self.value.read();
        self.codec.decode(&raw)
    }

    /// Write raw value.
    pub fn write_raw(&self, data: &[u8]) -> KnxResult<()> {
        if !self.flags.write {
            return Err(KnxError::GroupObjectWriteNotAllowed(self.name.clone()));
        }

        *self.value.write() = data.to_vec();
        *self.last_update.write() = std::time::Instant::now();
        Ok(())
    }

    /// Write decoded value.
    pub fn write_value(&self, value: &DptValue) -> KnxResult<()> {
        if !self.flags.write {
            return Err(KnxError::GroupObjectWriteNotAllowed(self.name.clone()));
        }

        let encoded = self.codec.encode(value)?;
        *self.value.write() = encoded;
        *self.last_update.write() = std::time::Instant::now();
        Ok(())
    }

    /// Check if read is allowed.
    pub fn can_read(&self) -> bool {
        self.flags.communication && self.flags.read
    }

    /// Check if write is allowed.
    pub fn can_write(&self) -> bool {
        self.flags.communication && self.flags.write
    }

    /// Get last update time.
    pub fn last_update(&self) -> std::time::Instant {
        *self.last_update.read()
    }

    /// Get age since last update.
    pub fn age(&self) -> std::time::Duration {
        self.last_update.read().elapsed()
    }
}

// ============================================================================
// Group Event
// ============================================================================

/// Event emitted when group object state changes.
#[derive(Debug, Clone)]
pub enum GroupEvent {
    /// Value was written.
    ValueWrite {
        address: GroupAddress,
        value: Vec<u8>,
        source: Option<String>,
    },
    /// Read request received.
    ReadRequest {
        address: GroupAddress,
        source: Option<String>,
    },
    /// Read response sent.
    ReadResponse {
        address: GroupAddress,
        value: Vec<u8>,
    },
    /// Object created.
    ObjectCreated { address: GroupAddress },
    /// Object removed.
    ObjectRemoved { address: GroupAddress },
}

impl GroupEvent {
    /// Get the address associated with this event.
    pub fn address(&self) -> GroupAddress {
        match self {
            Self::ValueWrite { address, .. } => *address,
            Self::ReadRequest { address, .. } => *address,
            Self::ReadResponse { address, .. } => *address,
            Self::ObjectCreated { address } => *address,
            Self::ObjectRemoved { address } => *address,
        }
    }
}

// ============================================================================
// Group Object Table
// ============================================================================

/// Group object table for managing multiple group objects.
pub struct GroupObjectTable {
    /// Objects indexed by group address.
    objects: DashMap<GroupAddress, Arc<GroupObject>>,
    /// Event broadcaster.
    event_tx: broadcast::Sender<GroupEvent>,
    /// DPT registry for creating objects.
    dpt_registry: Arc<DptRegistry>,
}

impl GroupObjectTable {
    /// Create a new group object table.
    pub fn new() -> Self {
        Self::with_registry(Arc::new(DptRegistry::new()))
    }

    /// Create with custom DPT registry.
    pub fn with_registry(registry: Arc<DptRegistry>) -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            objects: DashMap::new(),
            event_tx: tx,
            dpt_registry: registry,
        }
    }

    /// Subscribe to group events.
    pub fn subscribe(&self) -> broadcast::Receiver<GroupEvent> {
        self.event_tx.subscribe()
    }

    /// Add a group object.
    pub fn add(&self, object: GroupObject) {
        let address = object.address;
        self.objects.insert(address, Arc::new(object));
        let _ = self.event_tx.send(GroupEvent::ObjectCreated { address });
    }

    /// Create and add a group object.
    pub fn create(
        &self,
        address: GroupAddress,
        name: impl Into<String>,
        dpt_id: &DptId,
    ) -> KnxResult<Arc<GroupObject>> {
        let codec = self
            .dpt_registry
            .get(dpt_id)
            .ok_or_else(|| KnxError::InvalidDpt(format!("Unknown DPT: {}", dpt_id)))?;

        let object = Arc::new(GroupObject::new(address, name, codec));
        self.objects.insert(address, object.clone());
        let _ = self.event_tx.send(GroupEvent::ObjectCreated { address });

        Ok(object)
    }

    /// Remove a group object.
    pub fn remove(&self, address: &GroupAddress) -> Option<Arc<GroupObject>> {
        let removed = self.objects.remove(address).map(|(_, v)| v);
        if removed.is_some() {
            let _ = self
                .event_tx
                .send(GroupEvent::ObjectRemoved { address: *address });
        }
        removed
    }

    /// Get a group object.
    pub fn get(&self, address: &GroupAddress) -> Option<Arc<GroupObject>> {
        self.objects.get(address).map(|v| v.clone())
    }

    /// Check if address exists.
    pub fn contains(&self, address: &GroupAddress) -> bool {
        self.objects.contains_key(address)
    }

    /// Get all addresses.
    pub fn addresses(&self) -> Vec<GroupAddress> {
        self.objects.iter().map(|r| *r.key()).collect()
    }

    /// Get count of objects.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Read value from group object.
    pub fn read(&self, address: &GroupAddress) -> KnxResult<Vec<u8>> {
        let obj = self
            .get(address)
            .ok_or_else(|| KnxError::GroupObjectNotFound(address.to_string()))?;

        if !obj.can_read() {
            return Err(KnxError::GroupObjectReadNotAllowed(obj.name().to_string()));
        }

        Ok(obj.read_raw())
    }

    /// Read decoded value.
    pub fn read_value(&self, address: &GroupAddress) -> KnxResult<DptValue> {
        let obj = self
            .get(address)
            .ok_or_else(|| KnxError::GroupObjectNotFound(address.to_string()))?;

        if !obj.can_read() {
            return Err(KnxError::GroupObjectReadNotAllowed(obj.name().to_string()));
        }

        obj.read_value()
    }

    /// Write value to group object.
    pub fn write(
        &self,
        address: &GroupAddress,
        data: &[u8],
        source: Option<String>,
    ) -> KnxResult<()> {
        let obj = self
            .get(address)
            .ok_or_else(|| KnxError::GroupObjectNotFound(address.to_string()))?;

        obj.write_raw(data)?;

        let _ = self.event_tx.send(GroupEvent::ValueWrite {
            address: *address,
            value: data.to_vec(),
            source,
        });

        Ok(())
    }

    /// Write decoded value.
    pub fn write_value(
        &self,
        address: &GroupAddress,
        value: &DptValue,
        source: Option<String>,
    ) -> KnxResult<()> {
        let obj = self
            .get(address)
            .ok_or_else(|| KnxError::GroupObjectNotFound(address.to_string()))?;

        obj.write_value(value)?;

        let _ = self.event_tx.send(GroupEvent::ValueWrite {
            address: *address,
            value: obj.read_raw(),
            source,
        });

        Ok(())
    }

    /// Handle read request (record event, return value).
    pub fn handle_read_request(
        &self,
        address: &GroupAddress,
        source: Option<String>,
    ) -> KnxResult<Vec<u8>> {
        let _ = self.event_tx.send(GroupEvent::ReadRequest {
            address: *address,
            source,
        });

        let value = self.read(address)?;

        let _ = self.event_tx.send(GroupEvent::ReadResponse {
            address: *address,
            value: value.clone(),
        });

        Ok(value)
    }

    /// Iterate over all objects.
    pub fn iter(&self) -> impl Iterator<Item = Arc<GroupObject>> + '_ {
        self.objects.iter().map(|r| r.value().clone())
    }

    /// Get objects matching a predicate.
    pub fn filter<F>(&self, predicate: F) -> Vec<Arc<GroupObject>>
    where
        F: Fn(&GroupObject) -> bool,
    {
        self.objects
            .iter()
            .filter(|r| predicate(r.value()))
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get objects in address range.
    pub fn range(&self, start: GroupAddress, end: GroupAddress) -> Vec<Arc<GroupObject>> {
        self.objects
            .iter()
            .filter(|r| {
                let addr = r.key().raw();
                addr >= start.raw() && addr <= end.raw()
            })
            .map(|r| r.value().clone())
            .collect()
    }
}

impl Default for GroupObjectTable {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for creating group objects.
pub struct GroupObjectBuilder {
    address: GroupAddress,
    name: String,
    description: String,
    dpt_id: DptId,
    flags: GroupObjectFlags,
    initial_value: Option<DptValue>,
}

impl GroupObjectBuilder {
    /// Create a new builder.
    pub fn new(address: GroupAddress, dpt_id: DptId) -> Self {
        Self {
            address,
            name: format!("Group {}", address),
            description: String::new(),
            dpt_id,
            flags: GroupObjectFlags::read_write(),
            initial_value: None,
        }
    }

    /// Set name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set flags.
    pub fn flags(mut self, flags: GroupObjectFlags) -> Self {
        self.flags = flags;
        self
    }

    /// Set read-only.
    pub fn read_only(mut self) -> Self {
        self.flags = GroupObjectFlags::read_only();
        self
    }

    /// Set write-only.
    pub fn write_only(mut self) -> Self {
        self.flags = GroupObjectFlags::write_only();
        self
    }

    /// Set initial value.
    pub fn initial_value(mut self, value: DptValue) -> Self {
        self.initial_value = Some(value);
        self
    }

    /// Build the group object.
    pub fn build(self, registry: &DptRegistry) -> KnxResult<GroupObject> {
        let codec = registry
            .get(&self.dpt_id)
            .ok_or_else(|| KnxError::InvalidDpt(format!("Unknown DPT: {}", self.dpt_id)))?;

        let mut obj = GroupObject::new(self.address, self.name, codec)
            .with_description(self.description)
            .with_flags(self.flags);

        if let Some(value) = self.initial_value {
            obj = obj.with_value(&value)?;
        }

        Ok(obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dpt::DptId;

    #[test]
    fn test_group_object_read_write() {
        let registry = DptRegistry::new();
        let codec = registry.get(&DptId::new(9, 1)).unwrap();

        let obj = GroupObject::new(GroupAddress::three_level(1, 2, 3), "Temperature", codec);

        obj.write_value(&DptValue::F16(25.5)).unwrap();

        let value = obj.read_value().unwrap();
        if let DptValue::F16(v) = value {
            assert!((v - 25.5).abs() < 0.1);
        } else {
            panic!("Expected F16");
        }
    }

    #[test]
    fn test_group_object_flags() {
        let registry = DptRegistry::new();
        let codec = registry.get(&DptId::new(1, 1)).unwrap();

        let obj = GroupObject::new(GroupAddress::three_level(1, 0, 1), "Switch", codec)
            .with_flags(GroupObjectFlags::read_only());

        assert!(obj.can_read());
        assert!(!obj.can_write());

        let result = obj.write_value(&DptValue::Bool(true));
        assert!(result.is_err());
    }

    #[test]
    fn test_group_object_table() {
        let table = GroupObjectTable::new();

        let addr1 = GroupAddress::three_level(1, 0, 1);
        let addr2 = GroupAddress::three_level(1, 0, 2);

        table.create(addr1, "Light 1", &DptId::new(1, 1)).unwrap();
        table.create(addr2, "Light 2", &DptId::new(1, 1)).unwrap();

        assert_eq!(table.len(), 2);
        assert!(table.contains(&addr1));

        table.write(&addr1, &[1], None).unwrap();
        let value = table.read(&addr1).unwrap();
        assert_eq!(value, vec![1]);
    }

    #[test]
    fn test_group_object_builder() {
        let registry = DptRegistry::new();

        // Test read-only object (no initial value since it's read-only from outside)
        let obj = GroupObjectBuilder::new(GroupAddress::three_level(2, 0, 1), DptId::new(9, 1))
            .name("Room Temperature")
            .description("Living room temperature sensor")
            .read_only()
            .build(&registry)
            .unwrap();

        assert_eq!(obj.name(), "Room Temperature");
        assert!(obj.can_read());
        assert!(!obj.can_write());

        // Test read-write object with initial value
        let obj2 = GroupObjectBuilder::new(GroupAddress::three_level(2, 0, 2), DptId::new(9, 1))
            .name("Setpoint")
            .initial_value(DptValue::F16(22.0))
            .build(&registry)
            .unwrap();

        assert_eq!(obj2.name(), "Setpoint");
        assert!(obj2.can_read());
        assert!(obj2.can_write());
    }

    #[test]
    fn test_group_event_subscription() {
        let table = GroupObjectTable::new();
        let mut rx = table.subscribe();

        let addr = GroupAddress::three_level(1, 0, 1);
        table.create(addr, "Test", &DptId::new(1, 1)).unwrap();

        // Should receive ObjectCreated event
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, GroupEvent::ObjectCreated { .. }));
    }
}
