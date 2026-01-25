//! Register store configuration types.

use serde::{Deserialize, Serialize};

use super::RegisterType;

/// Address range configuration for a register type.
///
/// Defines the valid address range (inclusive) for a specific register type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressRange {
    /// Start address (inclusive).
    pub start: u16,
    /// End address (inclusive).
    pub end: u16,
}

impl AddressRange {
    /// Create a new address range.
    ///
    /// # Panics
    ///
    /// Panics if `start > end`.
    #[inline]
    pub const fn new(start: u16, end: u16) -> Self {
        assert!(start <= end, "start must be <= end");
        Self { start, end }
    }

    /// Create an address range from 0 to the specified count - 1.
    #[inline]
    pub const fn from_count(count: u16) -> Self {
        Self {
            start: 0,
            end: count.saturating_sub(1),
        }
    }

    /// Get the number of addresses in this range.
    #[inline]
    pub const fn count(&self) -> u32 {
        (self.end as u32) - (self.start as u32) + 1
    }

    /// Check if an address is within this range.
    #[inline]
    pub const fn contains(&self, address: u16) -> bool {
        address >= self.start && address <= self.end
    }

    /// Check if an address range is fully within this range.
    #[inline]
    pub const fn contains_range(&self, start: u16, count: u16) -> bool {
        if count == 0 {
            return false;
        }
        let end = match start.checked_add(count.saturating_sub(1)) {
            Some(e) => e,
            None => return false,
        };
        start >= self.start && end <= self.end
    }

    /// Validate an address range and return an error if invalid.
    pub fn validate(&self, start: u16, count: u16) -> Result<(), AddressRangeError> {
        if count == 0 {
            return Err(AddressRangeError::ZeroQuantity);
        }

        let end = start
            .checked_add(count.saturating_sub(1))
            .ok_or(AddressRangeError::Overflow)?;

        if start < self.start {
            return Err(AddressRangeError::BelowMinimum {
                address: start,
                minimum: self.start,
            });
        }

        if end > self.end {
            return Err(AddressRangeError::AboveMaximum {
                address: end,
                maximum: self.end,
            });
        }

        Ok(())
    }
}

impl Default for AddressRange {
    fn default() -> Self {
        Self::new(0, 9999)
    }
}

/// Address range validation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AddressRangeError {
    #[error("Zero quantity not allowed")]
    ZeroQuantity,

    #[error("Address overflow")]
    Overflow,

    #[error("Address {address} below minimum {minimum}")]
    BelowMinimum { address: u16, minimum: u16 },

    #[error("Address {address} above maximum {maximum}")]
    AboveMaximum { address: u16, maximum: u16 },
}

/// Configuration for a single register type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterRangeConfig {
    /// Address range for this register type.
    pub range: AddressRange,
    /// Default value for uninitialized registers.
    pub default_value: DefaultValue,
    /// Whether this register type is enabled.
    pub enabled: bool,
}

impl RegisterRangeConfig {
    /// Create a new register range configuration.
    pub const fn new(range: AddressRange, default_value: DefaultValue) -> Self {
        Self {
            range,
            default_value,
            enabled: true,
        }
    }

    /// Create a disabled configuration.
    pub const fn disabled() -> Self {
        Self {
            range: AddressRange::new(0, 0),
            default_value: DefaultValue::Zero,
            enabled: false,
        }
    }

    /// Enable/disable this register type.
    pub const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl Default for RegisterRangeConfig {
    fn default() -> Self {
        Self::new(AddressRange::default(), DefaultValue::Zero)
    }
}

/// Default value for uninitialized registers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultValue {
    /// Zero for words, false for bits.
    Zero,
    /// Specific value (as u16, converted to bool for bit types).
    Value(u16),
    /// Random value on first access.
    Random,
    /// Random value within a range (min, max).
    RandomRange(u16, u16),
}

impl DefaultValue {
    /// Get the default value for a bit type register.
    #[inline]
    pub fn get_bool(&self) -> bool {
        match self {
            Self::Zero => false,
            Self::Value(v) => *v != 0,
            Self::Random | Self::RandomRange(_, _) => rand_bool(),
        }
    }

    /// Get the default value for a word type register.
    #[inline]
    pub fn get_word(&self) -> u16 {
        match self {
            Self::Zero => 0,
            Self::Value(v) => *v,
            Self::Random => rand_u16(),
            Self::RandomRange(min, max) => rand_range(*min, *max),
        }
    }
}

impl Default for DefaultValue {
    fn default() -> Self {
        Self::Zero
    }
}

/// Initialization mode for the register store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitializationMode {
    /// Initialize all registers in range to default values on creation.
    ///
    /// This pre-allocates all registers, using more memory but ensuring
    /// deterministic behavior.
    Eager,

    /// Initialize registers lazily on first access.
    ///
    /// This is the default mode, providing memory efficiency for sparse access patterns.
    Lazy,

    /// Pre-initialize with a repeating byte pattern.
    ///
    /// Useful for testing and deterministic simulation.
    Pattern(Vec<u8>),

    /// Pre-initialize from a snapshot.
    ///
    /// Useful for loading saved state or starting from a known configuration.
    Snapshot(RegisterSnapshot),
}

impl Default for InitializationMode {
    fn default() -> Self {
        Self::Lazy
    }
}

/// Snapshot of register values for initialization or persistence.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RegisterSnapshot {
    /// Coil values (address -> value).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coils: Vec<(u16, bool)>,

    /// Discrete input values (address -> value).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub discrete_inputs: Vec<(u16, bool)>,

    /// Holding register values (address -> value).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holding_registers: Vec<(u16, u16)>,

    /// Input register values (address -> value).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_registers: Vec<(u16, u16)>,
}

/// Complete configuration for a register store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterStoreConfig {
    /// Coils configuration.
    pub coils: RegisterRangeConfig,

    /// Discrete inputs configuration.
    pub discrete_inputs: RegisterRangeConfig,

    /// Holding registers configuration.
    pub holding_registers: RegisterRangeConfig,

    /// Input registers configuration.
    pub input_registers: RegisterRangeConfig,

    /// Initialization mode.
    pub initialization: InitializationMode,

    /// Enable callback notifications (may impact performance).
    #[serde(default = "default_true")]
    pub callbacks_enabled: bool,
}

fn default_true() -> bool {
    true
}

impl RegisterStoreConfig {
    /// Create a new configuration with specified ranges.
    pub fn new(
        coils_range: AddressRange,
        discrete_inputs_range: AddressRange,
        holding_registers_range: AddressRange,
        input_registers_range: AddressRange,
    ) -> Self {
        Self {
            coils: RegisterRangeConfig::new(coils_range, DefaultValue::Zero),
            discrete_inputs: RegisterRangeConfig::new(discrete_inputs_range, DefaultValue::Zero),
            holding_registers: RegisterRangeConfig::new(
                holding_registers_range,
                DefaultValue::Zero,
            ),
            input_registers: RegisterRangeConfig::new(input_registers_range, DefaultValue::Zero),
            initialization: InitializationMode::default(),
            callbacks_enabled: true,
        }
    }

    /// Create a configuration with all ranges set to (0, max).
    pub fn with_max_addresses(max: u16) -> Self {
        let range = AddressRange::new(0, max);
        Self::new(range, range, range, range)
    }

    /// Create a minimal configuration (small ranges, for testing).
    pub fn minimal() -> Self {
        let range = AddressRange::new(0, 99);
        Self::new(range, range, range, range)
    }

    /// Create a large configuration (for stress testing).
    pub fn large() -> Self {
        let range = AddressRange::new(0, 65534);
        Self::new(range, range, range, range)
    }

    /// Create a large-scale configuration optimized for many units.
    ///
    /// Uses moderate ranges to balance memory usage across many units.
    pub fn large_scale() -> Self {
        let coils = AddressRange::new(0, 9999);
        let discrete = AddressRange::new(0, 9999);
        let holding = AddressRange::new(0, 9999);
        let input = AddressRange::new(0, 9999);

        Self::new(coils, discrete, holding, input).without_callbacks()
    }

    /// Set the initialization mode.
    pub fn with_initialization(mut self, mode: InitializationMode) -> Self {
        self.initialization = mode;
        self
    }

    /// Disable callbacks for better performance.
    pub fn without_callbacks(mut self) -> Self {
        self.callbacks_enabled = false;
        self
    }

    /// Get the configuration for a specific register type.
    pub fn get_range_config(&self, reg_type: RegisterType) -> &RegisterRangeConfig {
        match reg_type {
            RegisterType::Coil => &self.coils,
            RegisterType::DiscreteInput => &self.discrete_inputs,
            RegisterType::HoldingRegister => &self.holding_registers,
            RegisterType::InputRegister => &self.input_registers,
        }
    }

    /// Get the address range for a specific register type.
    pub fn get_range(&self, reg_type: RegisterType) -> AddressRange {
        self.get_range_config(reg_type).range
    }

    /// Validate an address range for a specific register type.
    pub fn validate_range(
        &self,
        reg_type: RegisterType,
        start: u16,
        count: u16,
    ) -> Result<(), AddressRangeError> {
        self.get_range(reg_type).validate(start, count)
    }
}

impl Default for RegisterStoreConfig {
    fn default() -> Self {
        Self::with_max_addresses(9999)
    }
}

// Helper functions for random values
fn rand_bool() -> bool {
    // Simple PRNG using thread-local state would be better,
    // but for now use a simple approach
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() % 2 == 0)
        .unwrap_or(false)
}

fn rand_u16() -> u16 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_nanos() & 0xFFFF) as u16)
        .unwrap_or(0)
}

fn rand_range(min: u16, max: u16) -> u16 {
    if min >= max {
        return min;
    }
    let range = (max - min) as u32 + 1;
    let random = rand_u16() as u32;
    min + (random % range) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_range() {
        let range = AddressRange::new(0, 99);
        assert_eq!(range.count(), 100);
        assert!(range.contains(0));
        assert!(range.contains(99));
        assert!(!range.contains(100));

        assert!(range.contains_range(0, 100));
        assert!(range.contains_range(50, 50));
        assert!(!range.contains_range(50, 51));
        assert!(!range.contains_range(0, 101));
    }

    #[test]
    fn test_address_range_validation() {
        let range = AddressRange::new(10, 99);

        assert!(range.validate(10, 1).is_ok());
        assert!(range.validate(10, 90).is_ok());
        assert!(range.validate(99, 1).is_ok());

        assert!(matches!(
            range.validate(0, 1),
            Err(AddressRangeError::BelowMinimum { .. })
        ));
        assert!(matches!(
            range.validate(100, 1),
            Err(AddressRangeError::AboveMaximum { .. })
        ));
        assert!(matches!(
            range.validate(10, 0),
            Err(AddressRangeError::ZeroQuantity)
        ));
    }

    #[test]
    fn test_default_value() {
        assert!(!DefaultValue::Zero.get_bool());
        assert_eq!(DefaultValue::Zero.get_word(), 0);

        assert!(DefaultValue::Value(1).get_bool());
        assert!(!DefaultValue::Value(0).get_bool());
        assert_eq!(DefaultValue::Value(12345).get_word(), 12345);
    }

    #[test]
    fn test_config_default() {
        let config = RegisterStoreConfig::default();
        assert_eq!(config.coils.range.start, 0);
        assert_eq!(config.coils.range.end, 9999);
        assert!(config.callbacks_enabled);
        assert_eq!(config.initialization, InitializationMode::Lazy);
    }

    #[test]
    fn test_config_presets() {
        let minimal = RegisterStoreConfig::minimal();
        assert_eq!(minimal.coils.range.count(), 100);

        let large = RegisterStoreConfig::large();
        assert_eq!(large.coils.range.count(), 65535);
    }
}
