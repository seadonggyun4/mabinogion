//! Object registry for managing BACnet objects.
//!
//! Provides thread-safe storage and lookup of BACnet objects
//! with support for high-volume scenarios.

use std::sync::Arc;

use dashmap::DashMap;

use super::property::{BACnetValue, PropertyId};
use super::traits::{ArcObject, BACnetObject};
use super::types::{ObjectId, ObjectType};

/// Registry for storing and managing BACnet objects.
///
/// This registry provides concurrent access to objects using DashMap
/// for lock-free reads in most cases.
pub struct ObjectRegistry {
    /// Objects indexed by ObjectId.
    objects: DashMap<ObjectId, ArcObject>,
    /// Objects indexed by name for name-based lookup.
    names: DashMap<String, ObjectId>,
    /// Object counts by type.
    type_counts: DashMap<ObjectType, usize>,
}

impl ObjectRegistry {
    /// Create a new empty object registry.
    pub fn new() -> Self {
        Self {
            objects: DashMap::new(),
            names: DashMap::new(),
            type_counts: DashMap::new(),
        }
    }

    /// Create a registry with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            objects: DashMap::with_capacity(capacity),
            names: DashMap::with_capacity(capacity),
            type_counts: DashMap::new(),
        }
    }

    /// Register an object.
    ///
    /// Returns the previous object if one existed with the same ID.
    pub fn register(&self, object: ArcObject) -> Option<ArcObject> {
        let id = object.object_identifier();
        let name = object.object_name().to_string();
        let object_type = id.object_type;

        // Remove old name mapping if replacing
        if let Some((_, old)) = self.objects.remove(&id) {
            self.names.remove(old.object_name());
            self.type_counts.entry(object_type).and_modify(|c| *c = c.saturating_sub(1));
        }

        // Add new mappings
        self.names.insert(name, id);
        self.type_counts.entry(object_type).and_modify(|c| *c += 1).or_insert(1);

        self.objects.insert(id, object)
    }

    /// Register a boxed object (convenience method).
    pub fn register_boxed(&self, object: Box<dyn BACnetObject>) -> Option<ArcObject> {
        self.register(Arc::from(object))
    }

    /// Unregister an object by ID.
    pub fn unregister(&self, id: &ObjectId) -> Option<ArcObject> {
        if let Some((_, object)) = self.objects.remove(id) {
            self.names.remove(object.object_name());
            self.type_counts.entry(id.object_type).and_modify(|c| *c = c.saturating_sub(1));
            Some(object)
        } else {
            None
        }
    }

    /// Get an object by ID.
    pub fn get(&self, id: &ObjectId) -> Option<ArcObject> {
        self.objects.get(id).map(|r| r.clone())
    }

    /// Get an object by name.
    pub fn get_by_name(&self, name: &str) -> Option<ArcObject> {
        self.names
            .get(name)
            .and_then(|id| self.objects.get(&id).map(|r| r.clone()))
    }

    /// Check if an object exists.
    pub fn contains(&self, id: &ObjectId) -> bool {
        self.objects.contains_key(id)
    }

    /// Check if an object with the given name exists.
    pub fn contains_name(&self, name: &str) -> bool {
        self.names.contains_key(name)
    }

    /// Get the total number of objects.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Get all object IDs.
    pub fn object_ids(&self) -> Vec<ObjectId> {
        self.objects.iter().map(|r| *r.key()).collect()
    }

    /// Get all object IDs of a specific type.
    pub fn object_ids_by_type(&self, object_type: ObjectType) -> Vec<ObjectId> {
        self.objects
            .iter()
            .filter(|r| r.key().object_type == object_type)
            .map(|r| *r.key())
            .collect()
    }

    /// Get the count of objects of a specific type.
    pub fn count_by_type(&self, object_type: ObjectType) -> usize {
        self.type_counts.get(&object_type).map(|r| *r).unwrap_or(0)
    }

    /// Get all object names.
    pub fn object_names(&self) -> Vec<String> {
        self.names.iter().map(|r| r.key().clone()).collect()
    }

    /// Read a property from an object.
    pub fn read_property(
        &self,
        object_id: &ObjectId,
        property_id: PropertyId,
    ) -> Result<BACnetValue, RegistryError> {
        let object = self
            .get(object_id)
            .ok_or(RegistryError::ObjectNotFound(*object_id))?;

        object
            .read_property(property_id)
            .map_err(|e| RegistryError::PropertyError(e.to_string()))
    }

    /// Write a property to an object.
    pub fn write_property(
        &self,
        object_id: &ObjectId,
        property_id: PropertyId,
        value: BACnetValue,
    ) -> Result<(), RegistryError> {
        let object = self
            .get(object_id)
            .ok_or(RegistryError::ObjectNotFound(*object_id))?;

        object
            .write_property(property_id, value)
            .map_err(|e| RegistryError::PropertyError(e.to_string()))
    }

    /// Iterate over all objects.
    pub fn iter(&self) -> impl Iterator<Item = ArcObject> + '_ {
        self.objects.iter().map(|r| r.clone())
    }

    /// Iterate over objects of a specific type.
    pub fn iter_by_type(&self, object_type: ObjectType) -> impl Iterator<Item = ArcObject> + '_ {
        self.objects
            .iter()
            .filter(move |r| r.key().object_type == object_type)
            .map(|r| r.clone())
    }

    /// Clear all objects from the registry.
    pub fn clear(&self) {
        self.objects.clear();
        self.names.clear();
        self.type_counts.clear();
    }

    /// Get statistics about the registry.
    pub fn statistics(&self) -> RegistryStatistics {
        let mut type_counts = Vec::new();
        for entry in self.type_counts.iter() {
            type_counts.push((*entry.key(), *entry.value()));
        }
        type_counts.sort_by_key(|(t, _)| *t as u16);

        RegistryStatistics {
            total_objects: self.objects.len(),
            type_counts,
        }
    }
}

impl Default for ObjectRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry statistics.
#[derive(Debug, Clone)]
pub struct RegistryStatistics {
    /// Total number of objects.
    pub total_objects: usize,
    /// Object counts by type.
    pub type_counts: Vec<(ObjectType, usize)>,
}

/// Registry errors.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("Object not found: {0}")]
    ObjectNotFound(ObjectId),

    #[error("Object name already exists: {0}")]
    NameExists(String),

    #[error("Property error: {0}")]
    PropertyError(String),
}

#[cfg(test)]
mod tests {
    use super::super::standard::{AnalogInput, BinaryOutput};
    use super::*;

    #[test]
    fn test_registry_basic_operations() {
        let registry = ObjectRegistry::new();

        let ai = Arc::new(AnalogInput::new(1, "Temperature"));
        let bo = Arc::new(BinaryOutput::new(1, "Fan"));

        registry.register(ai.clone());
        registry.register(bo.clone());

        assert_eq!(registry.len(), 2);
        assert!(registry.contains(&ObjectId::new(ObjectType::AnalogInput, 1)));
        assert!(registry.contains(&ObjectId::new(ObjectType::BinaryOutput, 1)));
    }

    #[test]
    fn test_registry_lookup_by_name() {
        let registry = ObjectRegistry::new();

        let ai = Arc::new(AnalogInput::new(1, "Zone Temperature"));
        registry.register(ai);

        let found = registry.get_by_name("Zone Temperature");
        assert!(found.is_some());
        assert_eq!(found.unwrap().object_name(), "Zone Temperature");

        assert!(registry.get_by_name("Nonexistent").is_none());
    }

    #[test]
    fn test_registry_type_counts() {
        let registry = ObjectRegistry::new();

        registry.register(Arc::new(AnalogInput::new(1, "AI1")));
        registry.register(Arc::new(AnalogInput::new(2, "AI2")));
        registry.register(Arc::new(BinaryOutput::new(1, "BO1")));

        assert_eq!(registry.count_by_type(ObjectType::AnalogInput), 2);
        assert_eq!(registry.count_by_type(ObjectType::BinaryOutput), 1);
        assert_eq!(registry.count_by_type(ObjectType::AnalogValue), 0);
    }

    #[test]
    fn test_registry_unregister() {
        let registry = ObjectRegistry::new();

        let ai = Arc::new(AnalogInput::new(1, "Temperature"));
        let id = ai.object_identifier();
        registry.register(ai);

        assert!(registry.contains(&id));
        let removed = registry.unregister(&id);
        assert!(removed.is_some());
        assert!(!registry.contains(&id));
        assert!(!registry.contains_name("Temperature"));
    }

    #[test]
    fn test_registry_property_access() {
        let registry = ObjectRegistry::new();

        let ai = Arc::new(AnalogInput::new(1, "Temperature"));
        ai.set_value(25.0);
        let id = ai.object_identifier();
        registry.register(ai);

        let value = registry.read_property(&id, PropertyId::PresentValue).unwrap();
        assert_eq!(value.as_real(), Some(25.0));
    }
}
