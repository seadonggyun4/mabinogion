//! High-performance sparse register store implementation.
//!
//! This module provides the core `SparseRegisterStore` that uses DashMap for
//! concurrent, memory-efficient storage of register values.

use std::sync::Arc;

use dashmap::DashMap;
use tracing::{instrument, trace};

use crate::error::{ModbusError, ModbusResult};

use super::callback::{CallbackManager, ReadCallback, ReadContext, WriteCallback, WriteContext};
use super::config::{InitializationMode, RegisterSnapshot, RegisterStoreConfig};
use super::types::RegisterType;
use super::value::RegisterValue;

/// High-performance sparse register store.
///
/// Uses DashMap for lock-free concurrent access, storing only registers
/// that have been accessed. This makes it suitable for simulating
/// large address spaces with sparse access patterns.
///
/// # Performance Characteristics
///
/// - Single register read: O(1), typically < 100ns
/// - Single register write: O(1), typically < 200ns
/// - Batch operations: O(n) where n is the number of registers
/// - Memory: Only stores accessed registers (~24 bytes per entry)
///
/// # Thread Safety
///
/// All operations are thread-safe and can be called concurrently from
/// multiple threads without external synchronization.
///
/// # Example
///
/// ```rust
/// use mabi_modbus::registers::{SparseRegisterStore, RegisterStoreConfig};
///
/// let store = SparseRegisterStore::new(RegisterStoreConfig::default());
///
/// // Write and read
/// store.write_holding_register(100, 12345).unwrap();
/// let values = store.read_holding_registers(100, 1).unwrap();
/// assert_eq!(values[0], 12345);
///
/// // Sparse storage - only accessed registers use memory
/// println!("Entries: {}", store.entry_count());
/// ```
pub struct SparseRegisterStore {
    /// Configuration.
    config: RegisterStoreConfig,

    /// Coils storage (sparse).
    coils: DashMap<u16, bool>,

    /// Discrete inputs storage (sparse).
    discrete_inputs: DashMap<u16, bool>,

    /// Holding registers storage (sparse).
    holding_registers: DashMap<u16, u16>,

    /// Input registers storage (sparse).
    input_registers: DashMap<u16, u16>,

    /// Callback manager.
    callbacks: Arc<CallbackManager>,
}

impl SparseRegisterStore {
    /// Create a new sparse register store with the given configuration.
    pub fn new(config: RegisterStoreConfig) -> Self {
        let store = Self {
            coils: DashMap::new(),
            discrete_inputs: DashMap::new(),
            holding_registers: DashMap::new(),
            input_registers: DashMap::new(),
            callbacks: Arc::new(CallbackManager::new()),
            config,
        };

        // Apply initialization mode
        store.apply_initialization();

        store
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(RegisterStoreConfig::default())
    }

    /// Apply initialization based on configuration.
    fn apply_initialization(&self) {
        match &self.config.initialization {
            InitializationMode::Lazy => {
                // No pre-initialization needed
            }
            InitializationMode::Eager => {
                self.initialize_eager();
            }
            InitializationMode::Pattern(pattern) => {
                self.initialize_pattern(pattern);
            }
            InitializationMode::Snapshot(snapshot) => {
                self.load_snapshot(snapshot);
            }
        }

        // Set callback enabled state from config
        self.callbacks.set_enabled(self.config.callbacks_enabled);
    }

    /// Eager initialization - pre-populate all registers.
    fn initialize_eager(&self) {
        // Initialize coils
        let coils_config = &self.config.coils;
        if coils_config.enabled {
            for addr in coils_config.range.start..=coils_config.range.end {
                let value = coils_config.default_value.get_bool();
                self.coils.insert(addr, value);
            }
        }

        // Initialize discrete inputs
        let di_config = &self.config.discrete_inputs;
        if di_config.enabled {
            for addr in di_config.range.start..=di_config.range.end {
                let value = di_config.default_value.get_bool();
                self.discrete_inputs.insert(addr, value);
            }
        }

        // Initialize holding registers
        let hr_config = &self.config.holding_registers;
        if hr_config.enabled {
            for addr in hr_config.range.start..=hr_config.range.end {
                let value = hr_config.default_value.get_word();
                self.holding_registers.insert(addr, value);
            }
        }

        // Initialize input registers
        let ir_config = &self.config.input_registers;
        if ir_config.enabled {
            for addr in ir_config.range.start..=ir_config.range.end {
                let value = ir_config.default_value.get_word();
                self.input_registers.insert(addr, value);
            }
        }
    }

    /// Pattern initialization.
    fn initialize_pattern(&self, pattern: &[u8]) {
        if pattern.is_empty() {
            return;
        }

        // Initialize holding registers with pattern
        let hr_config = &self.config.holding_registers;
        if hr_config.enabled {
            let mut pattern_idx = 0;
            for addr in hr_config.range.start..=hr_config.range.end {
                let hi = pattern[pattern_idx % pattern.len()];
                let lo = pattern[(pattern_idx + 1) % pattern.len()];
                let value = u16::from_be_bytes([hi, lo]);
                self.holding_registers.insert(addr, value);
                pattern_idx += 2;
            }
        }

        // Initialize input registers with pattern
        let ir_config = &self.config.input_registers;
        if ir_config.enabled {
            let mut pattern_idx = 0;
            for addr in ir_config.range.start..=ir_config.range.end {
                let hi = pattern[pattern_idx % pattern.len()];
                let lo = pattern[(pattern_idx + 1) % pattern.len()];
                let value = u16::from_be_bytes([hi, lo]);
                self.input_registers.insert(addr, value);
                pattern_idx += 2;
            }
        }
    }

    /// Load from snapshot.
    fn load_snapshot(&self, snapshot: &RegisterSnapshot) {
        for (addr, value) in &snapshot.coils {
            self.coils.insert(*addr, *value);
        }
        for (addr, value) in &snapshot.discrete_inputs {
            self.discrete_inputs.insert(*addr, *value);
        }
        for (addr, value) in &snapshot.holding_registers {
            self.holding_registers.insert(*addr, *value);
        }
        for (addr, value) in &snapshot.input_registers {
            self.input_registers.insert(*addr, *value);
        }
    }

    // =========================================================================
    // Configuration and Callbacks
    // =========================================================================

    /// Get the store configuration.
    pub fn config(&self) -> &RegisterStoreConfig {
        &self.config
    }

    /// Get the callback manager.
    pub fn callbacks(&self) -> &Arc<CallbackManager> {
        &self.callbacks
    }

    /// Add a read callback.
    pub fn add_read_callback(&self, callback: Arc<dyn ReadCallback>) {
        self.callbacks.add_read_callback(callback);
    }

    /// Add a write callback.
    pub fn add_write_callback(&self, callback: Arc<dyn WriteCallback>) {
        self.callbacks.add_write_callback(callback);
    }

    /// Enable or disable callbacks.
    pub fn set_callbacks_enabled(&self, enabled: bool) {
        self.callbacks.set_enabled(enabled);
    }

    // =========================================================================
    // Validation Helpers
    // =========================================================================

    /// Validate address range for a register type.
    #[inline]
    fn validate(&self, reg_type: RegisterType, address: u16, quantity: u16) -> ModbusResult<()> {
        self.config
            .validate_range(reg_type, address, quantity)
            .map_err(|e| match e {
                super::config::AddressRangeError::ZeroQuantity => {
                    ModbusError::InvalidQuantity { quantity: 0, max: 1 }
                }
                super::config::AddressRangeError::Overflow => ModbusError::InvalidAddress {
                    address,
                    max: u16::MAX,
                },
                super::config::AddressRangeError::BelowMinimum { address, .. }
                | super::config::AddressRangeError::AboveMaximum { address, .. } => {
                    let range = self.config.get_range(reg_type);
                    ModbusError::InvalidAddress {
                        address,
                        max: range.end,
                    }
                }
            })
    }

    /// Get default value for a register type.
    #[inline]
    fn get_default(&self, reg_type: RegisterType) -> RegisterValue {
        let config = self.config.get_range_config(reg_type);
        if reg_type.is_bit_type() {
            RegisterValue::Bool(config.default_value.get_bool())
        } else {
            RegisterValue::Word(config.default_value.get_word())
        }
    }

    // =========================================================================
    // Coil Operations
    // =========================================================================

    /// Read coils.
    ///
    /// # Arguments
    ///
    /// * `address` - Start address
    /// * `quantity` - Number of coils to read (1-2000)
    ///
    /// # Returns
    ///
    /// Vector of boolean values.
    #[instrument(level = "trace", skip(self))]
    pub fn read_coils(&self, address: u16, quantity: u16) -> ModbusResult<Vec<bool>> {
        self.validate(RegisterType::Coil, address, quantity)?;

        let default = self.config.coils.default_value.get_bool();
        let mut result = Vec::with_capacity(quantity as usize);
        let mut values_for_callback = Vec::with_capacity(quantity as usize);

        for i in 0..quantity {
            let addr = address + i;
            let value = self.coils.get(&addr).map(|v| *v).unwrap_or(default);
            result.push(value);
            values_for_callback.push(RegisterValue::Bool(value));
        }

        // Notify callbacks
        if self.callbacks.is_enabled() {
            let ctx = ReadContext {
                register_type: RegisterType::Coil,
                address,
                count: quantity,
            };
            self.callbacks.notify_read(ctx, &values_for_callback);
        }

        trace!(address, quantity, "Read coils");
        Ok(result)
    }

    /// Write a single coil.
    ///
    /// # Arguments
    ///
    /// * `address` - Coil address
    /// * `value` - Boolean value to write
    #[instrument(level = "trace", skip(self))]
    pub fn write_coil(&self, address: u16, value: bool) -> ModbusResult<()> {
        self.validate(RegisterType::Coil, address, 1)?;

        let old_value = self.coils.insert(address, value).unwrap_or(false);

        // Notify callbacks
        if self.callbacks.is_enabled() {
            let ctx = WriteContext {
                register_type: RegisterType::Coil,
                address,
                old_value: RegisterValue::Bool(old_value),
                new_value: RegisterValue::Bool(value),
            };
            self.callbacks.notify_write(ctx);
        }

        trace!(address, value, "Wrote coil");
        Ok(())
    }

    /// Write multiple coils.
    ///
    /// # Arguments
    ///
    /// * `address` - Start address
    /// * `values` - Boolean values to write
    #[instrument(level = "trace", skip(self, values))]
    pub fn write_coils(&self, address: u16, values: &[bool]) -> ModbusResult<()> {
        self.validate(RegisterType::Coil, address, values.len() as u16)?;

        for (i, &value) in values.iter().enumerate() {
            let addr = address + i as u16;
            let old_value = self.coils.insert(addr, value).unwrap_or(false);

            if self.callbacks.is_enabled() {
                let ctx = WriteContext {
                    register_type: RegisterType::Coil,
                    address: addr,
                    old_value: RegisterValue::Bool(old_value),
                    new_value: RegisterValue::Bool(value),
                };
                self.callbacks.notify_write(ctx);
            }
        }

        trace!(address, count = values.len(), "Wrote coils");
        Ok(())
    }

    // =========================================================================
    // Discrete Input Operations
    // =========================================================================

    /// Read discrete inputs.
    #[instrument(level = "trace", skip(self))]
    pub fn read_discrete_inputs(&self, address: u16, quantity: u16) -> ModbusResult<Vec<bool>> {
        self.validate(RegisterType::DiscreteInput, address, quantity)?;

        let default = self.config.discrete_inputs.default_value.get_bool();
        let mut result = Vec::with_capacity(quantity as usize);
        let mut values_for_callback = Vec::with_capacity(quantity as usize);

        for i in 0..quantity {
            let addr = address + i;
            let value = self.discrete_inputs.get(&addr).map(|v| *v).unwrap_or(default);
            result.push(value);
            values_for_callback.push(RegisterValue::Bool(value));
        }

        if self.callbacks.is_enabled() {
            let ctx = ReadContext {
                register_type: RegisterType::DiscreteInput,
                address,
                count: quantity,
            };
            self.callbacks.notify_read(ctx, &values_for_callback);
        }

        trace!(address, quantity, "Read discrete inputs");
        Ok(result)
    }

    /// Set a discrete input (simulator internal use).
    ///
    /// Discrete inputs are read-only from the Modbus client perspective,
    /// but the simulator needs to be able to set their values.
    #[instrument(level = "trace", skip(self))]
    pub fn set_discrete_input(&self, address: u16, value: bool) -> ModbusResult<()> {
        self.validate(RegisterType::DiscreteInput, address, 1)?;
        self.discrete_inputs.insert(address, value);
        trace!(address, value, "Set discrete input");
        Ok(())
    }

    /// Set multiple discrete inputs (simulator internal use).
    pub fn set_discrete_inputs(&self, address: u16, values: &[bool]) -> ModbusResult<()> {
        self.validate(RegisterType::DiscreteInput, address, values.len() as u16)?;
        for (i, &value) in values.iter().enumerate() {
            self.discrete_inputs.insert(address + i as u16, value);
        }
        Ok(())
    }

    // =========================================================================
    // Holding Register Operations
    // =========================================================================

    /// Read holding registers.
    ///
    /// # Arguments
    ///
    /// * `address` - Start address
    /// * `quantity` - Number of registers to read (1-125)
    #[instrument(level = "trace", skip(self))]
    pub fn read_holding_registers(&self, address: u16, quantity: u16) -> ModbusResult<Vec<u16>> {
        self.validate(RegisterType::HoldingRegister, address, quantity)?;

        let default = self.config.holding_registers.default_value.get_word();
        let mut result = Vec::with_capacity(quantity as usize);
        let mut values_for_callback = Vec::with_capacity(quantity as usize);

        for i in 0..quantity {
            let addr = address + i;
            let value = self.holding_registers.get(&addr).map(|v| *v).unwrap_or(default);
            result.push(value);
            values_for_callback.push(RegisterValue::Word(value));
        }

        if self.callbacks.is_enabled() {
            let ctx = ReadContext {
                register_type: RegisterType::HoldingRegister,
                address,
                count: quantity,
            };
            self.callbacks.notify_read(ctx, &values_for_callback);
        }

        trace!(address, quantity, "Read holding registers");
        Ok(result)
    }

    /// Write a single holding register.
    #[instrument(level = "trace", skip(self))]
    pub fn write_holding_register(&self, address: u16, value: u16) -> ModbusResult<()> {
        self.validate(RegisterType::HoldingRegister, address, 1)?;

        let old_value = self.holding_registers.insert(address, value).unwrap_or(0);

        if self.callbacks.is_enabled() {
            let ctx = WriteContext {
                register_type: RegisterType::HoldingRegister,
                address,
                old_value: RegisterValue::Word(old_value),
                new_value: RegisterValue::Word(value),
            };
            self.callbacks.notify_write(ctx);
        }

        trace!(address, value, "Wrote holding register");
        Ok(())
    }

    /// Write multiple holding registers.
    #[instrument(level = "trace", skip(self, values))]
    pub fn write_holding_registers(&self, address: u16, values: &[u16]) -> ModbusResult<()> {
        self.validate(RegisterType::HoldingRegister, address, values.len() as u16)?;

        for (i, &value) in values.iter().enumerate() {
            let addr = address + i as u16;
            let old_value = self.holding_registers.insert(addr, value).unwrap_or(0);

            if self.callbacks.is_enabled() {
                let ctx = WriteContext {
                    register_type: RegisterType::HoldingRegister,
                    address: addr,
                    old_value: RegisterValue::Word(old_value),
                    new_value: RegisterValue::Word(value),
                };
                self.callbacks.notify_write(ctx);
            }
        }

        trace!(address, count = values.len(), "Wrote holding registers");
        Ok(())
    }

    // =========================================================================
    // Input Register Operations
    // =========================================================================

    /// Read input registers.
    #[instrument(level = "trace", skip(self))]
    pub fn read_input_registers(&self, address: u16, quantity: u16) -> ModbusResult<Vec<u16>> {
        self.validate(RegisterType::InputRegister, address, quantity)?;

        let default = self.config.input_registers.default_value.get_word();
        let mut result = Vec::with_capacity(quantity as usize);
        let mut values_for_callback = Vec::with_capacity(quantity as usize);

        for i in 0..quantity {
            let addr = address + i;
            let value = self.input_registers.get(&addr).map(|v| *v).unwrap_or(default);
            result.push(value);
            values_for_callback.push(RegisterValue::Word(value));
        }

        if self.callbacks.is_enabled() {
            let ctx = ReadContext {
                register_type: RegisterType::InputRegister,
                address,
                count: quantity,
            };
            self.callbacks.notify_read(ctx, &values_for_callback);
        }

        trace!(address, quantity, "Read input registers");
        Ok(result)
    }

    /// Set an input register (simulator internal use).
    #[instrument(level = "trace", skip(self))]
    pub fn set_input_register(&self, address: u16, value: u16) -> ModbusResult<()> {
        self.validate(RegisterType::InputRegister, address, 1)?;
        self.input_registers.insert(address, value);
        trace!(address, value, "Set input register");
        Ok(())
    }

    /// Set multiple input registers (simulator internal use).
    pub fn set_input_registers(&self, address: u16, values: &[u16]) -> ModbusResult<()> {
        self.validate(RegisterType::InputRegister, address, values.len() as u16)?;
        for (i, &value) in values.iter().enumerate() {
            self.input_registers.insert(address + i as u16, value);
        }
        Ok(())
    }

    // =========================================================================
    // Byte-level Operations (for float/double support)
    // =========================================================================

    /// Read bytes from holding or input registers.
    ///
    /// Useful for reading multi-register values like f32 or f64.
    pub fn read_bytes(
        &self,
        reg_type: RegisterType,
        address: u16,
        byte_count: usize,
    ) -> ModbusResult<Vec<u8>> {
        let register_count = byte_count.div_ceil(2);

        let registers = match reg_type {
            RegisterType::HoldingRegister => {
                self.read_holding_registers(address, register_count as u16)?
            }
            RegisterType::InputRegister => {
                self.read_input_registers(address, register_count as u16)?
            }
            _ => {
                return Err(ModbusError::InvalidFunction(reg_type.read_function_code()));
            }
        };

        let mut bytes = Vec::with_capacity(byte_count);
        for reg in registers {
            bytes.extend_from_slice(&reg.to_be_bytes());
        }
        bytes.truncate(byte_count);

        Ok(bytes)
    }

    /// Write bytes to holding registers.
    ///
    /// Useful for writing multi-register values like f32 or f64.
    pub fn write_bytes(&self, address: u16, bytes: &[u8]) -> ModbusResult<()> {
        let mut registers = Vec::with_capacity((bytes.len() + 1) / 2);

        for chunk in bytes.chunks(2) {
            let value = if chunk.len() == 2 {
                u16::from_be_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_be_bytes([chunk[0], 0])
            };
            registers.push(value);
        }

        self.write_holding_registers(address, &registers)
    }

    /// Read f32 from holding registers (big-endian).
    pub fn read_f32(&self, address: u16) -> ModbusResult<f32> {
        let bytes = self.read_bytes(RegisterType::HoldingRegister, address, 4)?;
        Ok(f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Write f32 to holding registers (big-endian).
    pub fn write_f32(&self, address: u16, value: f32) -> ModbusResult<()> {
        self.write_bytes(address, &value.to_be_bytes())
    }

    /// Read f64 from holding registers (big-endian).
    pub fn read_f64(&self, address: u16) -> ModbusResult<f64> {
        let bytes = self.read_bytes(RegisterType::HoldingRegister, address, 8)?;
        Ok(f64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Write f64 to holding registers (big-endian).
    pub fn write_f64(&self, address: u16, value: f64) -> ModbusResult<()> {
        self.write_bytes(address, &value.to_be_bytes())
    }

    // =========================================================================
    // Utility Methods
    // =========================================================================

    /// Get the count of registers for a type (from configuration).
    pub fn count(&self, reg_type: RegisterType) -> u32 {
        self.config.get_range(reg_type).count()
    }

    /// Get the number of entries currently stored (across all types).
    pub fn entry_count(&self) -> usize {
        self.coils.len()
            + self.discrete_inputs.len()
            + self.holding_registers.len()
            + self.input_registers.len()
    }

    /// Get the number of entries for a specific register type.
    pub fn entry_count_for(&self, reg_type: RegisterType) -> usize {
        match reg_type {
            RegisterType::Coil => self.coils.len(),
            RegisterType::DiscreteInput => self.discrete_inputs.len(),
            RegisterType::HoldingRegister => self.holding_registers.len(),
            RegisterType::InputRegister => self.input_registers.len(),
        }
    }

    /// Estimate memory usage in bytes.
    ///
    /// This is an approximation based on DashMap overhead:
    /// - Each entry: ~24 bytes (key + value + overhead)
    /// - Base DashMap: ~1KB per map
    pub fn memory_usage(&self) -> usize {
        const DASHMAP_BASE: usize = 1024; // Base overhead per DashMap
        const BOOL_ENTRY_SIZE: usize = 24; // u16 key + bool value + overhead
        const WORD_ENTRY_SIZE: usize = 24; // u16 key + u16 value + overhead

        let coils_mem = DASHMAP_BASE + self.coils.len() * BOOL_ENTRY_SIZE;
        let di_mem = DASHMAP_BASE + self.discrete_inputs.len() * BOOL_ENTRY_SIZE;
        let hr_mem = DASHMAP_BASE + self.holding_registers.len() * WORD_ENTRY_SIZE;
        let ir_mem = DASHMAP_BASE + self.input_registers.len() * WORD_ENTRY_SIZE;

        coils_mem + di_mem + hr_mem + ir_mem
    }

    /// Reset all registers to default values.
    ///
    /// Clears all stored values, returning to lazy initialization state.
    pub fn reset(&self) {
        self.coils.clear();
        self.discrete_inputs.clear();
        self.holding_registers.clear();
        self.input_registers.clear();

        // Re-apply initialization
        self.apply_initialization();
    }

    /// Create a snapshot of current register values.
    pub fn snapshot(&self) -> RegisterSnapshot {
        RegisterSnapshot {
            coils: self.coils.iter().map(|e| (*e.key(), *e.value())).collect(),
            discrete_inputs: self
                .discrete_inputs
                .iter()
                .map(|e| (*e.key(), *e.value()))
                .collect(),
            holding_registers: self
                .holding_registers
                .iter()
                .map(|e| (*e.key(), *e.value()))
                .collect(),
            input_registers: self
                .input_registers
                .iter()
                .map(|e| (*e.key(), *e.value()))
                .collect(),
        }
    }

    /// Check if a specific register exists (has been accessed/written).
    pub fn exists(&self, reg_type: RegisterType, address: u16) -> bool {
        match reg_type {
            RegisterType::Coil => self.coils.contains_key(&address),
            RegisterType::DiscreteInput => self.discrete_inputs.contains_key(&address),
            RegisterType::HoldingRegister => self.holding_registers.contains_key(&address),
            RegisterType::InputRegister => self.input_registers.contains_key(&address),
        }
    }

    /// Remove a specific register (useful for testing sparse scenarios).
    pub fn remove(&self, reg_type: RegisterType, address: u16) -> bool {
        match reg_type {
            RegisterType::Coil => self.coils.remove(&address).is_some(),
            RegisterType::DiscreteInput => self.discrete_inputs.remove(&address).is_some(),
            RegisterType::HoldingRegister => self.holding_registers.remove(&address).is_some(),
            RegisterType::InputRegister => self.input_registers.remove(&address).is_some(),
        }
    }
}

impl Default for SparseRegisterStore {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl std::fmt::Debug for SparseRegisterStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SparseRegisterStore")
            .field("coils_entries", &self.coils.len())
            .field("discrete_inputs_entries", &self.discrete_inputs.len())
            .field("holding_registers_entries", &self.holding_registers.len())
            .field("input_registers_entries", &self.input_registers.len())
            .field("memory_usage", &self.memory_usage())
            .finish()
    }
}

// Thread safety markers
unsafe impl Send for SparseRegisterStore {}
unsafe impl Sync for SparseRegisterStore {}
