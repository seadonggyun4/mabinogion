//! Multi-unit manager implementation.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use tracing::{debug, instrument, trace, warn};

use crate::error::{ModbusError, ModbusResult};
use crate::registers::SparseRegisterStore;
use crate::types::{RegisterConverter, WordOrder};

use super::config::{BroadcastMode, UnitConfig, UnitManagerConfig};

/// Information about a managed Modbus unit.
#[derive(Debug)]
pub struct UnitInfo {
    /// Unit ID (1-247).
    unit_id: u8,

    /// Unit configuration.
    config: UnitConfig,

    /// Register store for this unit.
    registers: Arc<SparseRegisterStore>,

    /// Value converter for this unit.
    converter: RegisterConverter,

    /// Creation timestamp.
    created_at: Instant,

    /// Read request counter.
    read_count: AtomicU64,

    /// Write request counter.
    write_count: AtomicU64,

    /// Error counter.
    error_count: AtomicU64,
}

impl UnitInfo {
    /// Create a new unit info.
    fn new(
        unit_id: u8,
        config: UnitConfig,
        default_word_order: WordOrder,
        default_register_config: &crate::registers::RegisterStoreConfig,
    ) -> Self {
        let word_order = config.effective_word_order(default_word_order);
        let register_config = config
            .register_config
            .clone()
            .unwrap_or_else(|| default_register_config.clone());

        Self {
            unit_id,
            config,
            registers: Arc::new(SparseRegisterStore::new(register_config)),
            converter: RegisterConverter::new(word_order),
            created_at: Instant::now(),
            read_count: AtomicU64::new(0),
            write_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
        }
    }

    /// Get the unit ID.
    #[inline]
    pub fn unit_id(&self) -> u8 {
        self.unit_id
    }

    /// Get the unit configuration.
    #[inline]
    pub fn config(&self) -> &UnitConfig {
        &self.config
    }

    /// Get the unit name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Get the register store.
    #[inline]
    pub fn registers(&self) -> &Arc<SparseRegisterStore> {
        &self.registers
    }

    /// Get the value converter.
    #[inline]
    pub fn converter(&self) -> &RegisterConverter {
        &self.converter
    }

    /// Get the word order for this unit.
    #[inline]
    pub fn word_order(&self) -> WordOrder {
        self.converter.word_order()
    }

    /// Check if this unit is enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Check if this unit responds to broadcasts.
    #[inline]
    pub fn broadcast_enabled(&self) -> bool {
        self.config.broadcast_enabled
    }

    /// Get the response delay in microseconds.
    #[inline]
    pub fn response_delay_us(&self) -> u64 {
        self.config.response_delay_us
    }

    /// Get the creation timestamp.
    #[inline]
    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    /// Get the uptime duration.
    #[inline]
    pub fn uptime(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }

    /// Get the read count.
    #[inline]
    pub fn read_count(&self) -> u64 {
        self.read_count.load(Ordering::Relaxed)
    }

    /// Get the write count.
    #[inline]
    pub fn write_count(&self) -> u64 {
        self.write_count.load(Ordering::Relaxed)
    }

    /// Get the error count.
    #[inline]
    pub fn error_count(&self) -> u64 {
        self.error_count.load(Ordering::Relaxed)
    }

    /// Record a read operation.
    pub(crate) fn record_read(&self) {
        self.read_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a write operation.
    pub(crate) fn record_write(&self) {
        self.write_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an error.
    pub(crate) fn record_error(&self) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
    }
}

/// Manager for multiple Modbus units.
///
/// Provides centralized management of multiple Modbus slave units, each with
/// its own register store, word order configuration, and statistics.
///
/// # Thread Safety
///
/// `MultiUnitManager` is thread-safe and can be shared across threads.
/// All operations are lock-free using DashMap.
///
/// # Example
///
/// ```rust
/// use mabi_modbus::unit::{MultiUnitManager, UnitConfig, UnitManagerConfig};
///
/// let manager = MultiUnitManager::new(UnitManagerConfig::default());
///
/// // Add units
/// manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();
/// manager.add_unit(2, UnitConfig::new("Unit 2")).unwrap();
///
/// // Access unit
/// {
///     let unit = manager.get_unit(1).unwrap();
///     assert_eq!(unit.name(), "Unit 1");
/// }
/// ```
pub struct MultiUnitManager {
    /// Manager configuration.
    config: UnitManagerConfig,

    /// Units by ID.
    units: DashMap<u8, UnitInfo>,

    /// Total request counter.
    total_requests: AtomicU64,

    /// Total broadcast counter.
    broadcast_count: AtomicU64,
}

impl MultiUnitManager {
    /// Create a new multi-unit manager.
    pub fn new(config: UnitManagerConfig) -> Self {
        Self {
            config,
            units: DashMap::new(),
            total_requests: AtomicU64::new(0),
            broadcast_count: AtomicU64::new(0),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(UnitManagerConfig::default())
    }

    /// Get the manager configuration.
    pub fn config(&self) -> &UnitManagerConfig {
        &self.config
    }

    /// Get the number of registered units.
    pub fn unit_count(&self) -> usize {
        self.units.len()
    }

    /// Get all unit IDs.
    pub fn unit_ids(&self) -> Vec<u8> {
        self.units.iter().map(|entry| *entry.key()).collect()
    }

    /// Check if a unit exists.
    pub fn has_unit(&self, unit_id: u8) -> bool {
        self.units.contains_key(&unit_id)
    }

    // =========================================================================
    // Unit Management
    // =========================================================================

    /// Add a new unit.
    ///
    /// # Arguments
    ///
    /// * `unit_id` - Unit ID (1-247)
    /// * `config` - Unit configuration
    ///
    /// # Returns
    ///
    /// `Ok(())` if the unit was added, `Err` if:
    /// - Unit ID is 0 (reserved for broadcast)
    /// - Unit already exists
    /// - Maximum units reached
    #[instrument(skip(self, config), fields(unit_id = unit_id, name = %config.name))]
    pub fn add_unit(&self, unit_id: u8, config: UnitConfig) -> ModbusResult<()> {
        // Validate unit ID
        if unit_id == 0 {
            return Err(ModbusError::InvalidUnitId {
                unit_id: 0,
                reason: "Unit ID 0 is reserved for broadcast".to_string(),
            });
        }

        // Check max units
        if self.units.len() >= self.config.max_units {
            return Err(ModbusError::UnitLimitReached {
                max: self.config.max_units,
            });
        }

        // Check if already exists
        if self.units.contains_key(&unit_id) {
            return Err(ModbusError::UnitAlreadyExists { unit_id });
        }

        let unit_info = UnitInfo::new(
            unit_id,
            config,
            self.config.default_word_order,
            &self.config.default_register_config,
        );

        self.units.insert(unit_id, unit_info);
        debug!(unit_id, "Unit added");

        Ok(())
    }

    /// Remove a unit.
    ///
    /// # Returns
    ///
    /// The removed unit info if it existed.
    #[instrument(skip(self))]
    pub fn remove_unit(&self, unit_id: u8) -> Option<UnitInfo> {
        let removed = self.units.remove(&unit_id).map(|(_, info)| info);
        if removed.is_some() {
            debug!(unit_id, "Unit removed");
        }
        removed
    }

    /// Get a unit by ID.
    ///
    /// If `auto_create_units` is enabled and the unit doesn't exist,
    /// it will be created with default configuration.
    pub fn get_unit(&self, unit_id: u8) -> Option<dashmap::mapref::one::Ref<'_, u8, UnitInfo>> {
        // Never auto-create unit 0
        if unit_id == 0 {
            return None;
        }

        // Try to get existing unit
        if let Some(unit) = self.units.get(&unit_id) {
            return Some(unit);
        }

        // Auto-create if enabled
        if self.config.auto_create_units {
            let config = UnitConfig::new(format!("Auto-created Unit {}", unit_id));
            if self.add_unit(unit_id, config).is_ok() {
                return self.units.get(&unit_id);
            }
        }

        None
    }

    /// Get a mutable reference to a unit.
    pub fn get_unit_mut(
        &self,
        unit_id: u8,
    ) -> Option<dashmap::mapref::one::RefMut<'_, u8, UnitInfo>> {
        if unit_id == 0 {
            return None;
        }

        self.units.get_mut(&unit_id)
    }

    /// Update a unit's configuration.
    #[instrument(skip(self, update_fn))]
    pub fn update_unit<F>(&self, unit_id: u8, update_fn: F) -> ModbusResult<()>
    where
        F: FnOnce(&mut UnitConfig),
    {
        if let Some(mut unit) = self.units.get_mut(&unit_id) {
            update_fn(&mut unit.config);
            debug!(unit_id, "Unit configuration updated");
            Ok(())
        } else {
            Err(ModbusError::UnitNotFound { unit_id })
        }
    }

    // =========================================================================
    // Register Operations with Unit Selection
    // =========================================================================

    /// Read holding registers from a unit.
    #[instrument(skip(self), level = "trace")]
    pub fn read_holding_registers(
        &self,
        unit_id: u8,
        address: u16,
        quantity: u16,
    ) -> ModbusResult<Vec<u16>> {
        self.total_requests.fetch_add(1, Ordering::Relaxed);

        let unit = self
            .get_unit(unit_id)
            .ok_or(ModbusError::UnitNotFound { unit_id })?;

        if !unit.is_enabled() {
            unit.record_error();
            return Err(ModbusError::UnitDisabled { unit_id });
        }

        unit.record_read();
        unit.registers().read_holding_registers(address, quantity)
    }

    /// Read input registers from a unit.
    #[instrument(skip(self), level = "trace")]
    pub fn read_input_registers(
        &self,
        unit_id: u8,
        address: u16,
        quantity: u16,
    ) -> ModbusResult<Vec<u16>> {
        self.total_requests.fetch_add(1, Ordering::Relaxed);

        let unit = self
            .get_unit(unit_id)
            .ok_or(ModbusError::UnitNotFound { unit_id })?;

        if !unit.is_enabled() {
            unit.record_error();
            return Err(ModbusError::UnitDisabled { unit_id });
        }

        unit.record_read();
        unit.registers().read_input_registers(address, quantity)
    }

    /// Read coils from a unit.
    #[instrument(skip(self), level = "trace")]
    pub fn read_coils(
        &self,
        unit_id: u8,
        address: u16,
        quantity: u16,
    ) -> ModbusResult<Vec<bool>> {
        self.total_requests.fetch_add(1, Ordering::Relaxed);

        let unit = self
            .get_unit(unit_id)
            .ok_or(ModbusError::UnitNotFound { unit_id })?;

        if !unit.is_enabled() {
            unit.record_error();
            return Err(ModbusError::UnitDisabled { unit_id });
        }

        unit.record_read();
        unit.registers().read_coils(address, quantity)
    }

    /// Read discrete inputs from a unit.
    #[instrument(skip(self), level = "trace")]
    pub fn read_discrete_inputs(
        &self,
        unit_id: u8,
        address: u16,
        quantity: u16,
    ) -> ModbusResult<Vec<bool>> {
        self.total_requests.fetch_add(1, Ordering::Relaxed);

        let unit = self
            .get_unit(unit_id)
            .ok_or(ModbusError::UnitNotFound { unit_id })?;

        if !unit.is_enabled() {
            unit.record_error();
            return Err(ModbusError::UnitDisabled { unit_id });
        }

        unit.record_read();
        unit.registers().read_discrete_inputs(address, quantity)
    }

    /// Write a single holding register to a unit.
    #[instrument(skip(self), level = "trace")]
    pub fn write_holding_register(
        &self,
        unit_id: u8,
        address: u16,
        value: u16,
    ) -> ModbusResult<()> {
        self.total_requests.fetch_add(1, Ordering::Relaxed);

        // Handle broadcast
        if unit_id == 0 {
            return self.broadcast_write_holding_register(address, value);
        }

        let unit = self
            .get_unit(unit_id)
            .ok_or(ModbusError::UnitNotFound { unit_id })?;

        if !unit.is_enabled() {
            unit.record_error();
            return Err(ModbusError::UnitDisabled { unit_id });
        }

        unit.record_write();
        unit.registers().write_holding_register(address, value)
    }

    /// Write multiple holding registers to a unit.
    #[instrument(skip(self, values), level = "trace")]
    pub fn write_holding_registers(
        &self,
        unit_id: u8,
        address: u16,
        values: &[u16],
    ) -> ModbusResult<()> {
        self.total_requests.fetch_add(1, Ordering::Relaxed);

        // Handle broadcast
        if unit_id == 0 {
            return self.broadcast_write_holding_registers(address, values);
        }

        let unit = self
            .get_unit(unit_id)
            .ok_or(ModbusError::UnitNotFound { unit_id })?;

        if !unit.is_enabled() {
            unit.record_error();
            return Err(ModbusError::UnitDisabled { unit_id });
        }

        unit.record_write();
        unit.registers().write_holding_registers(address, values)
    }

    /// Write a single coil to a unit.
    #[instrument(skip(self), level = "trace")]
    pub fn write_coil(&self, unit_id: u8, address: u16, value: bool) -> ModbusResult<()> {
        self.total_requests.fetch_add(1, Ordering::Relaxed);

        // Handle broadcast
        if unit_id == 0 {
            return self.broadcast_write_coil(address, value);
        }

        let unit = self
            .get_unit(unit_id)
            .ok_or(ModbusError::UnitNotFound { unit_id })?;

        if !unit.is_enabled() {
            unit.record_error();
            return Err(ModbusError::UnitDisabled { unit_id });
        }

        unit.record_write();
        unit.registers().write_coil(address, value)
    }

    /// Write multiple coils to a unit.
    #[instrument(skip(self, values), level = "trace")]
    pub fn write_coils(
        &self,
        unit_id: u8,
        address: u16,
        values: &[bool],
    ) -> ModbusResult<()> {
        self.total_requests.fetch_add(1, Ordering::Relaxed);

        // Handle broadcast
        if unit_id == 0 {
            return self.broadcast_write_coils(address, values);
        }

        let unit = self
            .get_unit(unit_id)
            .ok_or(ModbusError::UnitNotFound { unit_id })?;

        if !unit.is_enabled() {
            unit.record_error();
            return Err(ModbusError::UnitDisabled { unit_id });
        }

        unit.record_write();
        unit.registers().write_coils(address, values)
    }

    // =========================================================================
    // Broadcast Operations
    // =========================================================================

    /// Broadcast write holding register to all applicable units.
    #[instrument(skip(self), level = "debug")]
    pub fn broadcast_write_holding_register(&self, address: u16, value: u16) -> ModbusResult<()> {
        self.broadcast_count.fetch_add(1, Ordering::Relaxed);
        let units = self.get_broadcast_targets();

        trace!(address, value, unit_count = units.len(), "Broadcasting write holding register");

        for unit_id in units {
            if let Some(unit) = self.units.get(&unit_id) {
                if unit.is_enabled() && unit.broadcast_enabled() {
                    let _ = unit.registers().write_holding_register(address, value);
                    unit.record_write();
                }
            }
        }

        Ok(())
    }

    /// Broadcast write multiple holding registers to all applicable units.
    #[instrument(skip(self, values), level = "debug")]
    pub fn broadcast_write_holding_registers(
        &self,
        address: u16,
        values: &[u16],
    ) -> ModbusResult<()> {
        self.broadcast_count.fetch_add(1, Ordering::Relaxed);
        let units = self.get_broadcast_targets();

        trace!(
            address,
            count = values.len(),
            unit_count = units.len(),
            "Broadcasting write multiple holding registers"
        );

        for unit_id in units {
            if let Some(unit) = self.units.get(&unit_id) {
                if unit.is_enabled() && unit.broadcast_enabled() {
                    let _ = unit.registers().write_holding_registers(address, values);
                    unit.record_write();
                }
            }
        }

        Ok(())
    }

    /// Broadcast write coil to all applicable units.
    #[instrument(skip(self), level = "debug")]
    pub fn broadcast_write_coil(&self, address: u16, value: bool) -> ModbusResult<()> {
        self.broadcast_count.fetch_add(1, Ordering::Relaxed);
        let units = self.get_broadcast_targets();

        trace!(address, value, unit_count = units.len(), "Broadcasting write coil");

        for unit_id in units {
            if let Some(unit) = self.units.get(&unit_id) {
                if unit.is_enabled() && unit.broadcast_enabled() {
                    let _ = unit.registers().write_coil(address, value);
                    unit.record_write();
                }
            }
        }

        Ok(())
    }

    /// Broadcast write multiple coils to all applicable units.
    #[instrument(skip(self, values), level = "debug")]
    pub fn broadcast_write_coils(&self, address: u16, values: &[bool]) -> ModbusResult<()> {
        self.broadcast_count.fetch_add(1, Ordering::Relaxed);
        let units = self.get_broadcast_targets();

        trace!(
            address,
            count = values.len(),
            unit_count = units.len(),
            "Broadcasting write multiple coils"
        );

        for unit_id in units {
            if let Some(unit) = self.units.get(&unit_id) {
                if unit.is_enabled() && unit.broadcast_enabled() {
                    let _ = unit.registers().write_coils(address, values);
                    unit.record_write();
                }
            }
        }

        Ok(())
    }

    /// Get the list of units that should receive broadcast messages.
    fn get_broadcast_targets(&self) -> Vec<u8> {
        match &self.config.broadcast_mode {
            BroadcastMode::WriteAll => self.unit_ids(),
            BroadcastMode::Disabled => vec![],
            BroadcastMode::SelectiveList(units) => units.clone(),
            BroadcastMode::EchoToUnit(id) => vec![*id],
        }
    }

    // =========================================================================
    // Statistics
    // =========================================================================

    /// Get total request count.
    pub fn total_requests(&self) -> u64 {
        self.total_requests.load(Ordering::Relaxed)
    }

    /// Get total broadcast count.
    pub fn broadcast_count(&self) -> u64 {
        self.broadcast_count.load(Ordering::Relaxed)
    }

    /// Get statistics for all units.
    pub fn unit_statistics(&self) -> Vec<UnitStatistics> {
        self.units
            .iter()
            .map(|entry| UnitStatistics {
                unit_id: *entry.key(),
                name: entry.value().config.name.clone(),
                enabled: entry.value().is_enabled(),
                read_count: entry.value().read_count(),
                write_count: entry.value().write_count(),
                error_count: entry.value().error_count(),
                register_count: entry.value().registers().entry_count(),
            })
            .collect()
    }

    /// Reset all statistics.
    pub fn reset_statistics(&self) {
        self.total_requests.store(0, Ordering::Relaxed);
        self.broadcast_count.store(0, Ordering::Relaxed);

        for entry in self.units.iter() {
            entry.value().read_count.store(0, Ordering::Relaxed);
            entry.value().write_count.store(0, Ordering::Relaxed);
            entry.value().error_count.store(0, Ordering::Relaxed);
        }
    }
}

impl Default for MultiUnitManager {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl std::fmt::Debug for MultiUnitManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiUnitManager")
            .field("unit_count", &self.unit_count())
            .field("total_requests", &self.total_requests())
            .field("broadcast_count", &self.broadcast_count())
            .finish()
    }
}

/// Statistics for a single unit.
#[derive(Debug, Clone)]
pub struct UnitStatistics {
    pub unit_id: u8,
    pub name: String,
    pub enabled: bool,
    pub read_count: u64,
    pub write_count: u64,
    pub error_count: u64,
    pub register_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_manager() {
        let manager = MultiUnitManager::with_defaults();
        assert_eq!(manager.unit_count(), 0);
    }

    #[test]
    fn test_add_and_get_unit() {
        let manager = MultiUnitManager::with_defaults();

        manager
            .add_unit(1, UnitConfig::new("Test Unit"))
            .unwrap();

        assert!(manager.has_unit(1));
        assert!(!manager.has_unit(2));

        let unit = manager.get_unit(1).unwrap();
        assert_eq!(unit.name(), "Test Unit");
    }

    #[test]
    fn test_cannot_add_unit_zero() {
        let manager = MultiUnitManager::with_defaults();

        let result = manager.add_unit(0, UnitConfig::new("Broadcast"));
        assert!(result.is_err());
    }

    #[test]
    fn test_cannot_add_duplicate_unit() {
        let manager = MultiUnitManager::with_defaults();

        manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();
        let result = manager.add_unit(1, UnitConfig::new("Unit 1 Again"));
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_unit() {
        let manager = MultiUnitManager::with_defaults();

        manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();
        assert!(manager.has_unit(1));

        let removed = manager.remove_unit(1);
        assert!(removed.is_some());
        assert!(!manager.has_unit(1));
    }

    #[test]
    fn test_read_write_operations() {
        let manager = MultiUnitManager::with_defaults();
        manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();

        // Write
        manager.write_holding_register(1, 0, 12345).unwrap();

        // Read
        let values = manager.read_holding_registers(1, 0, 1).unwrap();
        assert_eq!(values[0], 12345);
    }

    #[test]
    fn test_broadcast_write() {
        let manager = MultiUnitManager::with_defaults();
        manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();
        manager.add_unit(2, UnitConfig::new("Unit 2")).unwrap();
        manager.add_unit(3, UnitConfig::new("Unit 3")).unwrap();

        // Broadcast write (unit_id = 0)
        manager.write_holding_register(0, 100, 999).unwrap();

        // All units should have the value
        let v1 = manager.read_holding_registers(1, 100, 1).unwrap();
        let v2 = manager.read_holding_registers(2, 100, 1).unwrap();
        let v3 = manager.read_holding_registers(3, 100, 1).unwrap();

        assert_eq!(v1[0], 999);
        assert_eq!(v2[0], 999);
        assert_eq!(v3[0], 999);
    }

    #[test]
    fn test_broadcast_with_disabled_unit() {
        let manager = MultiUnitManager::with_defaults();
        manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();
        manager
            .add_unit(2, UnitConfig::new("Unit 2").with_broadcast(false))
            .unwrap();

        // Broadcast write
        manager.broadcast_write_holding_register(100, 888).unwrap();

        // Unit 1 should have the value
        let v1 = manager.read_holding_registers(1, 100, 1).unwrap();
        assert_eq!(v1[0], 888);

        // Unit 2 should NOT have the value (broadcast disabled)
        let v2 = manager.read_holding_registers(2, 100, 1).unwrap();
        assert_ne!(v2[0], 888);
    }

    #[test]
    fn test_auto_create_units() {
        let config = UnitManagerConfig::default().with_auto_create(true);
        let manager = MultiUnitManager::new(config);

        // Unit doesn't exist yet
        assert!(!manager.has_unit(5));

        // Access creates it
        let unit = manager.get_unit(5);
        assert!(unit.is_some());
        assert!(manager.has_unit(5));
    }

    #[test]
    fn test_statistics() {
        let manager = MultiUnitManager::with_defaults();
        manager.add_unit(1, UnitConfig::new("Unit 1")).unwrap();

        // Perform some operations
        manager.write_holding_register(1, 0, 100).unwrap();
        manager.read_holding_registers(1, 0, 1).unwrap();
        manager.read_holding_registers(1, 0, 1).unwrap();

        let stats = manager.unit_statistics();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].read_count, 2);
        assert_eq!(stats[0].write_count, 1);
    }

    #[test]
    fn test_different_word_orders() {
        let manager = MultiUnitManager::with_defaults();

        manager
            .add_unit(1, UnitConfig::with_word_order("BE Unit", WordOrder::BigEndian))
            .unwrap();
        manager
            .add_unit(
                2,
                UnitConfig::with_word_order("LE Unit", WordOrder::LittleEndian),
            )
            .unwrap();

        let unit1 = manager.get_unit(1).unwrap();
        let unit2 = manager.get_unit(2).unwrap();

        assert_eq!(unit1.word_order(), WordOrder::BigEndian);
        assert_eq!(unit2.word_order(), WordOrder::LittleEndian);
    }
}
