//! DPT Registry for dynamic codec lookup.
//!
//! The registry allows looking up DPT codecs by their ID at runtime,
//! supporting both standard and custom DPT types.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use super::codec::{BoxedDptCodec, DptCodec, DptId};
use super::types::*;
use crate::error::{KnxError, KnxResult};

/// Registry for DPT codecs.
///
/// Provides dynamic lookup of DPT codecs by their ID.
/// Pre-populated with standard DPT types.
///
/// # Example
///
/// ```rust,ignore
/// let registry = DptRegistry::new();
/// let codec = registry.get(&DptId::new(9, 1)).unwrap();
/// let value = codec.decode(&[0x0C, 0x8C]).unwrap();
/// ```
pub struct DptRegistry {
    codecs: RwLock<HashMap<DptId, Arc<BoxedDptCodec>>>,
    /// Fallback codecs for main types (when specific subtype not found).
    main_type_fallbacks: RwLock<HashMap<u16, Arc<BoxedDptCodec>>>,
}

impl DptRegistry {
    /// Create a new registry with standard DPT types pre-registered.
    pub fn new() -> Self {
        let registry = Self {
            codecs: RwLock::new(HashMap::new()),
            main_type_fallbacks: RwLock::new(HashMap::new()),
        };
        registry.register_standard_types();
        registry
    }

    /// Create an empty registry without standard types.
    pub fn empty() -> Self {
        Self {
            codecs: RwLock::new(HashMap::new()),
            main_type_fallbacks: RwLock::new(HashMap::new()),
        }
    }

    /// Register a DPT codec.
    pub fn register<C: DptCodec + 'static>(&self, codec: C) {
        let id = codec.id();
        let boxed: BoxedDptCodec = Box::new(codec);
        self.codecs.write().insert(id, Arc::new(boxed));
    }

    /// Register a boxed codec.
    pub fn register_boxed(&self, codec: BoxedDptCodec) {
        let id = codec.id();
        self.codecs.write().insert(id, Arc::new(codec));
    }

    /// Register a fallback codec for a main type.
    pub fn register_fallback<C: DptCodec + 'static>(&self, main_type: u16, codec: C) {
        let boxed: BoxedDptCodec = Box::new(codec);
        self.main_type_fallbacks
            .write()
            .insert(main_type, Arc::new(boxed));
    }

    /// Get a codec by ID.
    pub fn get(&self, id: &DptId) -> Option<Arc<BoxedDptCodec>> {
        // Try exact match first
        if let Some(codec) = self.codecs.read().get(id).cloned() {
            return Some(codec);
        }

        // Try fallback for main type
        self.main_type_fallbacks.read().get(&id.main).cloned()
    }

    /// Get a codec by ID, returning error if not found.
    pub fn get_or_err(&self, id: &DptId) -> KnxResult<Arc<BoxedDptCodec>> {
        self.get(id)
            .ok_or_else(|| KnxError::InvalidDpt(format!("Unknown DPT: {}", id)))
    }

    /// Get a codec by string ID (e.g., "DPT 9.001", "9.1", "DPT1").
    pub fn get_by_str(&self, id: &str) -> Option<Arc<BoxedDptCodec>> {
        id.parse::<DptId>().ok().and_then(|id| self.get(&id))
    }

    /// Check if a DPT is registered.
    pub fn contains(&self, id: &DptId) -> bool {
        self.codecs.read().contains_key(id)
            || self.main_type_fallbacks.read().contains_key(&id.main)
    }

    /// Get all registered DPT IDs.
    pub fn list_ids(&self) -> Vec<DptId> {
        self.codecs.read().keys().copied().collect()
    }

    /// Get count of registered codecs.
    pub fn len(&self) -> usize {
        self.codecs.read().len()
    }

    /// Check if registry is empty.
    pub fn is_empty(&self) -> bool {
        self.codecs.read().is_empty()
    }

    /// Register all standard DPT types.
    fn register_standard_types(&self) {
        // DPT 1: Boolean types
        self.register(Dpt1Switch);
        self.register(Dpt1Bool);
        self.register(Dpt1UpDown);

        // DPT 2: Priority control
        self.register(Dpt2SwitchControl);

        // DPT 3: Control types
        self.register(Dpt3DimmingControl);
        self.register(Dpt3BlindsControl);

        // DPT 5: Unsigned 8-bit
        self.register(Dpt5Scaling);
        self.register(Dpt5Angle);
        self.register(Dpt5Counter);

        // DPT 6: Signed 8-bit
        self.register(Dpt6Percent);

        // DPT 7: Unsigned 16-bit
        self.register(Dpt7Pulses);

        // DPT 8: Signed 16-bit
        self.register(Dpt8PulsesDiff);

        // DPT 9: 16-bit float
        self.register(Dpt9Temperature);
        self.register(Dpt9Lux);
        self.register(Dpt9Humidity);

        // DPT 12: Unsigned 32-bit
        self.register(Dpt12Counter);

        // DPT 13: Signed 32-bit
        self.register(Dpt13Counter);

        // DPT 14: 32-bit float
        self.register(Dpt14Float);

        // DPT 16: String
        self.register(Dpt16String);

        // DPT 17/18: Scene
        self.register(Dpt17Scene);
        self.register(Dpt18SceneControl);

        // DPT 20: HVAC Mode
        self.register(Dpt20HvacMode);

        // DPT 232: Color RGB
        self.register(Dpt232Rgb);

        // Register fallbacks for main types
        self.register_fallback(1, Dpt1Switch);
        self.register_fallback(5, Dpt5Counter);
        self.register_fallback(9, Dpt9Temperature);
        self.register_fallback(14, Dpt14Float);
    }
}

impl Default for DptRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Global registry instance
lazy_static::lazy_static! {
    /// Global DPT registry instance.
    pub static ref GLOBAL_DPT_REGISTRY: DptRegistry = DptRegistry::new();
}

#[cfg(test)]
/// Get a codec from the global registry.
pub fn get_codec(id: &DptId) -> Option<Arc<BoxedDptCodec>> {
    GLOBAL_DPT_REGISTRY.get(id)
}

#[cfg(test)]
/// Get a codec from the global registry by string.
pub fn get_codec_by_str(id: &str) -> Option<Arc<BoxedDptCodec>> {
    GLOBAL_DPT_REGISTRY.get_by_str(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dpt::DptValue;

    #[test]
    fn test_registry_get() {
        let registry = DptRegistry::new();

        let codec = registry.get(&DptId::new(9, 1)).unwrap();
        assert_eq!(codec.name(), "Temperature");
    }

    #[test]
    fn test_registry_get_by_str() {
        let registry = DptRegistry::new();

        assert!(registry.get_by_str("DPT 9.001").is_some());
        assert!(registry.get_by_str("9.1").is_some());
        assert!(registry.get_by_str("1").is_some());
    }

    #[test]
    fn test_registry_fallback() {
        let registry = DptRegistry::new();

        // DPT 9.999 doesn't exist, but should fall back to DPT 9 (Temperature)
        let codec = registry.get(&DptId::new(9, 999));
        assert!(codec.is_some());
    }

    #[test]
    fn test_registry_custom_codec() {
        let registry = DptRegistry::empty();

        // Define a custom codec
        struct MyDpt;
        impl DptCodec for MyDpt {
            fn id(&self) -> DptId {
                DptId::new(999, 1)
            }
            fn name(&self) -> &'static str {
                "My Custom DPT"
            }
            fn size(&self) -> usize {
                2
            }
            fn encode(&self, _value: &DptValue) -> KnxResult<Vec<u8>> {
                Ok(vec![0xAB, 0xCD])
            }
            fn decode(&self, _data: &[u8]) -> KnxResult<DptValue> {
                Ok(DptValue::U16(0xABCD))
            }
        }

        registry.register(MyDpt);

        let codec = registry.get(&DptId::new(999, 1)).unwrap();
        assert_eq!(codec.name(), "My Custom DPT");
    }

    #[test]
    fn test_global_registry() {
        let codec = get_codec(&DptId::new(1, 1)).unwrap();
        assert_eq!(codec.name(), "Switch");

        let codec = get_codec_by_str("DPT 9.001").unwrap();
        assert_eq!(codec.name(), "Temperature");
    }
}
