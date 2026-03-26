//! High-performance sparse register store implementation.
//!
//! This module provides the core `SparseRegisterStore` that uses a sharded,
//! segment-oriented sparse map for concurrent, memory-efficient storage.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::error::{ModbusError, ModbusResult};

use super::callback::{CallbackManager, ReadCallback, ReadContext, WriteCallback, WriteContext};
use super::config::{InitializationMode, RegisterSnapshot, RegisterStoreConfig};
use super::types::RegisterType;
use super::value::RegisterValue;

const SHARD_COUNT: usize = 64;
const SHARD_MASK: usize = SHARD_COUNT - 1;
const SHARD_SHIFT: usize = 6;
const SEGMENT_SIZE: usize = 64;
const SEGMENT_MASK: usize = SEGMENT_SIZE - 1;
const TOTAL_SEGMENTS: usize = (u16::MAX as usize + 1) / SEGMENT_SIZE;
const SEGMENTS_PER_SHARD: usize = TOTAL_SEGMENTS / SHARD_COUNT;

#[derive(Clone)]
struct RegisterSegment<T: Copy + Default> {
    values: [T; SEGMENT_SIZE],
    occupancy: u64,
}

impl<T: Copy + Default> RegisterSegment<T> {
    #[inline]
    fn new() -> Self {
        Self {
            values: [T::default(); SEGMENT_SIZE],
            occupancy: 0,
        }
    }

    #[inline]
    fn contains(&self, offset: usize) -> bool {
        (self.occupancy & (1u64 << offset)) != 0
    }

    #[inline]
    fn get(&self, offset: usize) -> Option<T> {
        self.contains(offset).then_some(self.values[offset])
    }

    #[inline]
    fn set(&mut self, offset: usize, value: T) -> Option<T> {
        let mask = 1u64 << offset;
        let previous = if (self.occupancy & mask) != 0 {
            Some(self.values[offset])
        } else {
            None
        };
        self.values[offset] = value;
        self.occupancy |= mask;
        previous
    }

    #[inline]
    fn remove(&mut self, offset: usize) -> Option<T> {
        let mask = 1u64 << offset;
        if (self.occupancy & mask) == 0 {
            return None;
        }

        self.occupancy &= !mask;
        Some(self.values[offset])
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.occupancy == 0
    }
}

struct ShardedRegisterMap<T: Copy + Default> {
    shards: Box<[RwLock<Box<[Option<RegisterSegment<T>>]>>]>,
    len: AtomicUsize,
}

impl<T: Copy + Default> ShardedRegisterMap<T> {
    fn new() -> Self {
        let shards = (0..SHARD_COUNT)
            .map(|_| RwLock::new(vec![None; SEGMENTS_PER_SHARD].into_boxed_slice()))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            shards,
            len: AtomicUsize::new(0),
        }
    }

    #[inline]
    fn split_address(address: u16) -> (u16, usize) {
        let raw = address as usize;
        ((raw / SEGMENT_SIZE) as u16, raw & SEGMENT_MASK)
    }

    #[inline]
    fn shard_index(segment: u16) -> usize {
        (segment as usize) & SHARD_MASK
    }

    #[inline]
    fn slot_index(segment: u16) -> usize {
        (segment as usize) >> SHARD_SHIFT
    }

    #[inline]
    fn next_boundary(address: usize) -> usize {
        ((address / SEGMENT_SIZE) + 1) * SEGMENT_SIZE
    }

    #[inline]
    fn insert(&self, address: u16, value: T) -> Option<T> {
        let (segment_key, offset) = Self::split_address(address);
        let mut shard = self.shards[Self::shard_index(segment_key)].write();
        let previous = shard[Self::slot_index(segment_key)]
            .get_or_insert_with(RegisterSegment::new)
            .set(offset, value);
        if previous.is_none() {
            self.len.fetch_add(1, Ordering::Relaxed);
        }
        previous
    }

    fn populate_range<F>(&self, start: u16, end_inclusive: u16, mut value_for: F)
    where
        F: FnMut(u16) -> T,
    {
        let end_exclusive = end_inclusive as usize + 1;
        let mut current = start as usize;

        while current < end_exclusive {
            let chunk_end = Self::next_boundary(current).min(end_exclusive);
            let (segment_key, start_offset) = Self::split_address(current as u16);
            let mut shard = self.shards[Self::shard_index(segment_key)].write();
            let segment =
                shard[Self::slot_index(segment_key)].get_or_insert_with(RegisterSegment::new);
            let mut inserted = 0usize;

            for raw_addr in current..chunk_end {
                let address = raw_addr as u16;
                let offset = start_offset + (raw_addr - current);
                if segment.set(offset, value_for(address)).is_none() {
                    inserted += 1;
                }
            }

            if inserted > 0 {
                self.len.fetch_add(inserted, Ordering::Relaxed);
            }
            current = chunk_end;
        }
    }

    fn read_range(&self, address: u16, quantity: u16, default: T) -> Vec<T> {
        let end_exclusive = address as usize + quantity as usize;
        let mut current = address as usize;
        let mut result = vec![default; quantity as usize];

        while current < end_exclusive {
            let chunk_end = Self::next_boundary(current).min(end_exclusive);
            let (segment_key, start_offset) = Self::split_address(current as u16);
            let shard = self.shards[Self::shard_index(segment_key)].read();

            if let Some(segment) = shard[Self::slot_index(segment_key)].as_ref() {
                let result_offset = current - address as usize;
                for raw_addr in current..chunk_end {
                    let segment_offset = start_offset + (raw_addr - current);
                    if let Some(value) = segment.get(segment_offset) {
                        result[result_offset + (raw_addr - current)] = value;
                    }
                }
            }

            current = chunk_end;
        }

        result
    }

    #[inline]
    fn read_one(&self, address: u16, default: T) -> T {
        let (segment_key, offset) = Self::split_address(address);
        let shard = self.shards[Self::shard_index(segment_key)].read();
        shard[Self::slot_index(segment_key)]
            .as_ref()
            .and_then(|segment| segment.get(offset))
            .unwrap_or(default)
    }

    fn write_range(&self, address: u16, values: &[T]) {
        if values.is_empty() {
            return;
        }

        let end_exclusive = address as usize + values.len();
        let mut current = address as usize;

        while current < end_exclusive {
            let chunk_end = Self::next_boundary(current).min(end_exclusive);
            let start_offset = current - address as usize;
            let end_offset = start_offset + (chunk_end - current);
            let (segment_key, segment_offset) = Self::split_address(current as u16);
            let mut shard = self.shards[Self::shard_index(segment_key)].write();
            let segment =
                shard[Self::slot_index(segment_key)].get_or_insert_with(RegisterSegment::new);
            let mut inserted = 0usize;

            for (offset, value) in values[start_offset..end_offset].iter().copied().enumerate() {
                if segment.set(segment_offset + offset, value).is_none() {
                    inserted += 1;
                }
            }

            if inserted > 0 {
                self.len.fetch_add(inserted, Ordering::Relaxed);
            }
            current = chunk_end;
        }
    }

    fn clear(&self) {
        for shard in self.shards.iter() {
            let mut shard = shard.write();
            for segment in shard.iter_mut() {
                *segment = None;
            }
        }
        self.len.store(0, Ordering::Relaxed);
    }

    fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    fn active_shard_count(&self) -> usize {
        self.shards
            .iter()
            .filter(|shard| shard.read().iter().any(|segment| segment.is_some()))
            .count()
    }

    fn contains_key(&self, address: u16) -> bool {
        let (segment_key, offset) = Self::split_address(address);
        let shard = self.shards[Self::shard_index(segment_key)].read();
        shard[Self::slot_index(segment_key)]
            .as_ref()
            .map(|segment| segment.contains(offset))
            .unwrap_or(false)
    }

    fn remove(&self, address: u16) -> Option<T> {
        let (segment_key, offset) = Self::split_address(address);
        let mut shard = self.shards[Self::shard_index(segment_key)].write();
        let slot = Self::slot_index(segment_key);
        let previous = shard[slot]
            .as_mut()
            .and_then(|segment| segment.remove(offset));
        if previous.is_some() {
            self.len.fetch_sub(1, Ordering::Relaxed);
            let remove_segment = shard[slot]
                .as_ref()
                .map(|segment| segment.is_empty())
                .unwrap_or(false);
            if remove_segment {
                shard[slot] = None;
            }
        }
        previous
    }

    fn snapshot(&self) -> Vec<(u16, T)> {
        let mut entries = Vec::with_capacity(self.len());
        for (shard_index, shard) in self.shards.iter().enumerate() {
            let shard = shard.read();
            for (slot_index, segment) in shard.iter().enumerate() {
                if let Some(segment) = segment.as_ref() {
                    let segment_key = (slot_index << SHARD_SHIFT) | shard_index;
                    let base = segment_key * SEGMENT_SIZE;
                    for offset in 0..SEGMENT_SIZE {
                        if let Some(value) = segment.get(offset) {
                            entries.push(((base + offset) as u16, value));
                        }
                    }
                }
            }
        }
        entries
    }
}

/// High-performance sparse register store.
///
/// Uses sharded sparse segments with block-local locking, storing only touched
/// segments and slots. This keeps sparse profiles memory efficient while
/// reducing contention and lookup overhead on hot concurrent paths.
pub struct SparseRegisterStore {
    /// Configuration.
    config: RegisterStoreConfig,

    /// Coils storage (sparse).
    coils: ShardedRegisterMap<bool>,

    /// Discrete inputs storage (sparse).
    discrete_inputs: ShardedRegisterMap<bool>,

    /// Holding registers storage (sparse).
    holding_registers: ShardedRegisterMap<u16>,

    /// Input registers storage (sparse).
    input_registers: ShardedRegisterMap<u16>,

    /// Callback manager.
    callbacks: Arc<CallbackManager>,
}

impl SparseRegisterStore {
    /// Create a new sparse register store with the given configuration.
    pub fn new(config: RegisterStoreConfig) -> Self {
        let store = Self {
            coils: ShardedRegisterMap::new(),
            discrete_inputs: ShardedRegisterMap::new(),
            holding_registers: ShardedRegisterMap::new(),
            input_registers: ShardedRegisterMap::new(),
            callbacks: Arc::new(CallbackManager::new()),
            config,
        };

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
            InitializationMode::Lazy => {}
            InitializationMode::Eager => self.initialize_eager(),
            InitializationMode::Pattern(pattern) => self.initialize_pattern(pattern),
            InitializationMode::Snapshot(snapshot) => self.load_snapshot(snapshot),
        }

        self.callbacks.set_enabled(self.config.callbacks_enabled);
    }

    /// Eager initialization - pre-populate all registers.
    fn initialize_eager(&self) {
        let coils_config = &self.config.coils;
        if coils_config.enabled {
            self.coils
                .populate_range(coils_config.range.start, coils_config.range.end, |_| {
                    coils_config.default_value.get_bool()
                });
        }

        let discrete_config = &self.config.discrete_inputs;
        if discrete_config.enabled {
            self.discrete_inputs.populate_range(
                discrete_config.range.start,
                discrete_config.range.end,
                |_| discrete_config.default_value.get_bool(),
            );
        }

        let holding_config = &self.config.holding_registers;
        if holding_config.enabled {
            self.holding_registers.populate_range(
                holding_config.range.start,
                holding_config.range.end,
                |_| holding_config.default_value.get_word(),
            );
        }

        let input_config = &self.config.input_registers;
        if input_config.enabled {
            self.input_registers.populate_range(
                input_config.range.start,
                input_config.range.end,
                |_| input_config.default_value.get_word(),
            );
        }
    }

    /// Pattern initialization.
    fn initialize_pattern(&self, pattern: &[u8]) {
        if pattern.is_empty() {
            return;
        }

        let holding_config = &self.config.holding_registers;
        if holding_config.enabled {
            let mut pattern_idx = 0usize;
            self.holding_registers.populate_range(
                holding_config.range.start,
                holding_config.range.end,
                |_| {
                    let hi = pattern[pattern_idx % pattern.len()];
                    let lo = pattern[(pattern_idx + 1) % pattern.len()];
                    pattern_idx += 2;
                    u16::from_be_bytes([hi, lo])
                },
            );
        }

        let input_config = &self.config.input_registers;
        if input_config.enabled {
            let mut pattern_idx = 0usize;
            self.input_registers.populate_range(
                input_config.range.start,
                input_config.range.end,
                |_| {
                    let hi = pattern[pattern_idx % pattern.len()];
                    let lo = pattern[(pattern_idx + 1) % pattern.len()];
                    pattern_idx += 2;
                    u16::from_be_bytes([hi, lo])
                },
            );
        }
    }

    /// Load from snapshot.
    fn load_snapshot(&self, snapshot: &RegisterSnapshot) {
        for (address, value) in &snapshot.coils {
            self.coils.insert(*address, *value);
        }
        for (address, value) in &snapshot.discrete_inputs {
            self.discrete_inputs.insert(*address, *value);
        }
        for (address, value) in &snapshot.holding_registers {
            self.holding_registers.insert(*address, *value);
        }
        for (address, value) in &snapshot.input_registers {
            self.input_registers.insert(*address, *value);
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
                super::config::AddressRangeError::ZeroQuantity => ModbusError::InvalidQuantity {
                    quantity: 0,
                    max: 1,
                },
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

    // =========================================================================
    // Coil Operations
    // =========================================================================

    /// Read coils.
    pub fn read_coils(&self, address: u16, quantity: u16) -> ModbusResult<Vec<bool>> {
        self.validate(RegisterType::Coil, address, quantity)?;

        let default = self.config.coils.default_value.get_bool();
        let result = if quantity == 1 {
            vec![self.coils.read_one(address, default)]
        } else {
            self.coils.read_range(address, quantity, default)
        };

        if self.callbacks.has_read_callbacks() {
            let values = result
                .iter()
                .copied()
                .map(RegisterValue::Bool)
                .collect::<Vec<_>>();
            self.callbacks.notify_read(
                ReadContext {
                    register_type: RegisterType::Coil,
                    address,
                    count: quantity,
                },
                &values,
            );
        }

        Ok(result)
    }

    /// Write a single coil.
    pub fn write_coil(&self, address: u16, value: bool) -> ModbusResult<()> {
        self.validate(RegisterType::Coil, address, 1)?;

        if self.callbacks.has_write_callbacks() {
            let old_value = self.coils.insert(address, value).unwrap_or(false);
            self.callbacks.notify_write(WriteContext {
                register_type: RegisterType::Coil,
                address,
                old_value: RegisterValue::Bool(old_value),
                new_value: RegisterValue::Bool(value),
            });
        } else {
            self.coils.insert(address, value);
        }

        Ok(())
    }

    /// Write multiple coils.
    pub fn write_coils(&self, address: u16, values: &[bool]) -> ModbusResult<()> {
        self.validate(RegisterType::Coil, address, values.len() as u16)?;

        if self.callbacks.has_write_callbacks() {
            for (offset, value) in values.iter().copied().enumerate() {
                let addr = address + offset as u16;
                let old_value = self.coils.insert(addr, value).unwrap_or(false);
                self.callbacks.notify_write(WriteContext {
                    register_type: RegisterType::Coil,
                    address: addr,
                    old_value: RegisterValue::Bool(old_value),
                    new_value: RegisterValue::Bool(value),
                });
            }
        } else {
            self.coils.write_range(address, values);
        }

        Ok(())
    }

    // =========================================================================
    // Discrete Input Operations
    // =========================================================================

    /// Read discrete inputs.
    pub fn read_discrete_inputs(&self, address: u16, quantity: u16) -> ModbusResult<Vec<bool>> {
        self.validate(RegisterType::DiscreteInput, address, quantity)?;

        let default = self.config.discrete_inputs.default_value.get_bool();
        let result = if quantity == 1 {
            vec![self.discrete_inputs.read_one(address, default)]
        } else {
            self.discrete_inputs.read_range(address, quantity, default)
        };

        if self.callbacks.has_read_callbacks() {
            let values = result
                .iter()
                .copied()
                .map(RegisterValue::Bool)
                .collect::<Vec<_>>();
            self.callbacks.notify_read(
                ReadContext {
                    register_type: RegisterType::DiscreteInput,
                    address,
                    count: quantity,
                },
                &values,
            );
        }

        Ok(result)
    }

    /// Set a discrete input (simulator internal use).
    pub fn set_discrete_input(&self, address: u16, value: bool) -> ModbusResult<()> {
        self.validate(RegisterType::DiscreteInput, address, 1)?;
        self.discrete_inputs.insert(address, value);
        Ok(())
    }

    /// Set multiple discrete inputs (simulator internal use).
    pub fn set_discrete_inputs(&self, address: u16, values: &[bool]) -> ModbusResult<()> {
        self.validate(RegisterType::DiscreteInput, address, values.len() as u16)?;
        self.discrete_inputs.write_range(address, values);
        Ok(())
    }

    // =========================================================================
    // Holding Register Operations
    // =========================================================================

    /// Read holding registers.
    pub fn read_holding_registers(&self, address: u16, quantity: u16) -> ModbusResult<Vec<u16>> {
        self.validate(RegisterType::HoldingRegister, address, quantity)?;

        let default = self.config.holding_registers.default_value.get_word();
        let result = if quantity == 1 {
            vec![self.holding_registers.read_one(address, default)]
        } else {
            self.holding_registers
                .read_range(address, quantity, default)
        };

        if self.callbacks.has_read_callbacks() {
            let values = result
                .iter()
                .copied()
                .map(RegisterValue::Word)
                .collect::<Vec<_>>();
            self.callbacks.notify_read(
                ReadContext {
                    register_type: RegisterType::HoldingRegister,
                    address,
                    count: quantity,
                },
                &values,
            );
        }

        Ok(result)
    }

    /// Write a single holding register.
    pub fn write_holding_register(&self, address: u16, value: u16) -> ModbusResult<()> {
        self.validate(RegisterType::HoldingRegister, address, 1)?;

        if self.callbacks.has_write_callbacks() {
            let old_value = self.holding_registers.insert(address, value).unwrap_or(0);
            self.callbacks.notify_write(WriteContext {
                register_type: RegisterType::HoldingRegister,
                address,
                old_value: RegisterValue::Word(old_value),
                new_value: RegisterValue::Word(value),
            });
        } else {
            self.holding_registers.insert(address, value);
        }

        Ok(())
    }

    /// Write multiple holding registers.
    pub fn write_holding_registers(&self, address: u16, values: &[u16]) -> ModbusResult<()> {
        self.validate(RegisterType::HoldingRegister, address, values.len() as u16)?;

        if self.callbacks.has_write_callbacks() {
            for (offset, value) in values.iter().copied().enumerate() {
                let addr = address + offset as u16;
                let old_value = self.holding_registers.insert(addr, value).unwrap_or(0);
                self.callbacks.notify_write(WriteContext {
                    register_type: RegisterType::HoldingRegister,
                    address: addr,
                    old_value: RegisterValue::Word(old_value),
                    new_value: RegisterValue::Word(value),
                });
            }
        } else {
            self.holding_registers.write_range(address, values);
        }

        Ok(())
    }

    // =========================================================================
    // Input Register Operations
    // =========================================================================

    /// Read input registers.
    pub fn read_input_registers(&self, address: u16, quantity: u16) -> ModbusResult<Vec<u16>> {
        self.validate(RegisterType::InputRegister, address, quantity)?;

        let default = self.config.input_registers.default_value.get_word();
        let result = if quantity == 1 {
            vec![self.input_registers.read_one(address, default)]
        } else {
            self.input_registers.read_range(address, quantity, default)
        };

        if self.callbacks.has_read_callbacks() {
            let values = result
                .iter()
                .copied()
                .map(RegisterValue::Word)
                .collect::<Vec<_>>();
            self.callbacks.notify_read(
                ReadContext {
                    register_type: RegisterType::InputRegister,
                    address,
                    count: quantity,
                },
                &values,
            );
        }

        Ok(result)
    }

    /// Set an input register (simulator internal use).
    pub fn set_input_register(&self, address: u16, value: u16) -> ModbusResult<()> {
        self.validate(RegisterType::InputRegister, address, 1)?;
        self.input_registers.insert(address, value);
        Ok(())
    }

    /// Set multiple input registers (simulator internal use).
    pub fn set_input_registers(&self, address: u16, values: &[u16]) -> ModbusResult<()> {
        self.validate(RegisterType::InputRegister, address, values.len() as u16)?;
        self.input_registers.write_range(address, values);
        Ok(())
    }

    // =========================================================================
    // Byte-level Operations (for float/double support)
    // =========================================================================

    /// Read bytes from holding or input registers.
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
        for register in registers {
            bytes.extend_from_slice(&register.to_be_bytes());
        }
        bytes.truncate(byte_count);

        Ok(bytes)
    }

    /// Write bytes to holding registers.
    pub fn write_bytes(&self, address: u16, bytes: &[u8]) -> ModbusResult<()> {
        let mut registers = Vec::with_capacity(bytes.len().div_ceil(2));

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
    pub fn memory_usage(&self) -> usize {
        const STORE_BASE: usize = 256;
        const ACTIVE_SHARD_BASE: usize = 24;
        const BOOL_ENTRY_SIZE: usize = 16;
        const WORD_ENTRY_SIZE: usize = 24;

        let coils_mem = self.coils.active_shard_count() * ACTIVE_SHARD_BASE
            + self.coils.len() * BOOL_ENTRY_SIZE;
        let discrete_mem = self.discrete_inputs.active_shard_count() * ACTIVE_SHARD_BASE
            + self.discrete_inputs.len() * BOOL_ENTRY_SIZE;
        let holding_mem = self.holding_registers.active_shard_count() * ACTIVE_SHARD_BASE
            + self.holding_registers.len() * WORD_ENTRY_SIZE;
        let input_mem = self.input_registers.active_shard_count() * ACTIVE_SHARD_BASE
            + self.input_registers.len() * WORD_ENTRY_SIZE;

        STORE_BASE + coils_mem + discrete_mem + holding_mem + input_mem
    }

    /// Reset all registers to default values.
    pub fn reset(&self) {
        self.coils.clear();
        self.discrete_inputs.clear();
        self.holding_registers.clear();
        self.input_registers.clear();
        self.apply_initialization();
    }

    /// Create a snapshot of current register values.
    pub fn snapshot(&self) -> RegisterSnapshot {
        RegisterSnapshot {
            coils: self.coils.snapshot(),
            discrete_inputs: self.discrete_inputs.snapshot(),
            holding_registers: self.holding_registers.snapshot(),
            input_registers: self.input_registers.snapshot(),
        }
    }

    /// Check if a specific register exists (has been accessed/written).
    pub fn exists(&self, reg_type: RegisterType, address: u16) -> bool {
        match reg_type {
            RegisterType::Coil => self.coils.contains_key(address),
            RegisterType::DiscreteInput => self.discrete_inputs.contains_key(address),
            RegisterType::HoldingRegister => self.holding_registers.contains_key(address),
            RegisterType::InputRegister => self.input_registers.contains_key(address),
        }
    }

    /// Remove a specific register (useful for testing sparse scenarios).
    pub fn remove(&self, reg_type: RegisterType, address: u16) -> bool {
        match reg_type {
            RegisterType::Coil => self.coils.remove(address).is_some(),
            RegisterType::DiscreteInput => self.discrete_inputs.remove(address).is_some(),
            RegisterType::HoldingRegister => self.holding_registers.remove(address).is_some(),
            RegisterType::InputRegister => self.input_registers.remove(address).is_some(),
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
