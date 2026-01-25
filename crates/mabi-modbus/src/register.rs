//! Modbus register storage.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::error::{ModbusError, ModbusResult};

/// Modbus register types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RegisterType {
    /// Coil (read/write, 1 bit).
    Coil,
    /// Discrete input (read-only, 1 bit).
    DiscreteInput,
    /// Holding register (read/write, 16 bits).
    HoldingRegister,
    /// Input register (read-only, 16 bits).
    InputRegister,
}

impl RegisterType {
    /// Get Modbus function code for reading.
    pub fn read_function(&self) -> u8 {
        match self {
            Self::Coil => 0x01,
            Self::DiscreteInput => 0x02,
            Self::HoldingRegister => 0x03,
            Self::InputRegister => 0x04,
        }
    }

    /// Get Modbus function code for writing (None for read-only types).
    pub fn write_function(&self) -> Option<u8> {
        match self {
            Self::Coil => Some(0x05),            // Write Single Coil
            Self::HoldingRegister => Some(0x06), // Write Single Register
            Self::DiscreteInput | Self::InputRegister => None,
        }
    }

    /// Get Modbus function code for writing multiple.
    pub fn write_multiple_function(&self) -> Option<u8> {
        match self {
            Self::Coil => Some(0x0F),            // Write Multiple Coils
            Self::HoldingRegister => Some(0x10), // Write Multiple Registers
            Self::DiscreteInput | Self::InputRegister => None,
        }
    }

    /// Check if this register type is writable.
    pub fn is_writable(&self) -> bool {
        matches!(self, Self::Coil | Self::HoldingRegister)
    }

    /// Check if this is a bit type (coil or discrete input).
    pub fn is_bit_type(&self) -> bool {
        matches!(self, Self::Coil | Self::DiscreteInput)
    }

    /// Maximum quantity for read operation.
    pub fn max_read_quantity(&self) -> u16 {
        if self.is_bit_type() {
            2000 // Max bits per read
        } else {
            125 // Max registers per read
        }
    }

    /// Maximum quantity for write operation.
    pub fn max_write_quantity(&self) -> u16 {
        if self.is_bit_type() {
            1968 // Max bits per write
        } else {
            123 // Max registers per write
        }
    }
}

/// Thread-safe Modbus register storage.
pub struct RegisterStore {
    /// Coils (bit values).
    coils: RwLock<Vec<bool>>,
    /// Discrete inputs (bit values).
    discrete_inputs: RwLock<Vec<bool>>,
    /// Holding registers (16-bit values).
    holding_registers: RwLock<Vec<u16>>,
    /// Input registers (16-bit values).
    input_registers: RwLock<Vec<u16>>,

    /// Sizes.
    coil_count: u16,
    discrete_input_count: u16,
    holding_register_count: u16,
    input_register_count: u16,
}

impl RegisterStore {
    /// Create a new register store with specified sizes.
    pub fn new(
        coils: u16,
        discrete_inputs: u16,
        holding_registers: u16,
        input_registers: u16,
    ) -> Self {
        Self {
            coils: RwLock::new(vec![false; coils as usize]),
            discrete_inputs: RwLock::new(vec![false; discrete_inputs as usize]),
            holding_registers: RwLock::new(vec![0u16; holding_registers as usize]),
            input_registers: RwLock::new(vec![0u16; input_registers as usize]),
            coil_count: coils,
            discrete_input_count: discrete_inputs,
            holding_register_count: holding_registers,
            input_register_count: input_registers,
        }
    }

    /// Create with default sizes (10000 each).
    pub fn with_defaults() -> Self {
        Self::new(10000, 10000, 10000, 10000)
    }

    /// Get the count for a register type.
    pub fn count(&self, reg_type: RegisterType) -> u16 {
        match reg_type {
            RegisterType::Coil => self.coil_count,
            RegisterType::DiscreteInput => self.discrete_input_count,
            RegisterType::HoldingRegister => self.holding_register_count,
            RegisterType::InputRegister => self.input_register_count,
        }
    }

    /// Validate address and quantity.
    fn validate(&self, reg_type: RegisterType, address: u16, quantity: u16) -> ModbusResult<()> {
        let max = self.count(reg_type);

        if address >= max {
            return Err(ModbusError::invalid_address(address, max - 1));
        }

        if quantity == 0 || address + quantity > max {
            return Err(ModbusError::invalid_quantity(quantity, max - address));
        }

        Ok(())
    }

    // === Coil operations ===

    /// Read coils.
    pub fn read_coils(&self, address: u16, quantity: u16) -> ModbusResult<Vec<bool>> {
        self.validate(RegisterType::Coil, address, quantity)?;

        let coils = self.coils.read();
        let start = address as usize;
        let end = start + quantity as usize;
        Ok(coils[start..end].to_vec())
    }

    /// Write a single coil.
    pub fn write_coil(&self, address: u16, value: bool) -> ModbusResult<()> {
        self.validate(RegisterType::Coil, address, 1)?;

        let mut coils = self.coils.write();
        coils[address as usize] = value;
        Ok(())
    }

    /// Write multiple coils.
    pub fn write_coils(&self, address: u16, values: &[bool]) -> ModbusResult<()> {
        self.validate(RegisterType::Coil, address, values.len() as u16)?;

        let mut coils = self.coils.write();
        for (i, &value) in values.iter().enumerate() {
            coils[address as usize + i] = value;
        }
        Ok(())
    }

    // === Discrete Input operations ===

    /// Read discrete inputs.
    pub fn read_discrete_inputs(&self, address: u16, quantity: u16) -> ModbusResult<Vec<bool>> {
        self.validate(RegisterType::DiscreteInput, address, quantity)?;

        let inputs = self.discrete_inputs.read();
        let start = address as usize;
        let end = start + quantity as usize;
        Ok(inputs[start..end].to_vec())
    }

    /// Set discrete input (internal use for simulation).
    pub fn set_discrete_input(&self, address: u16, value: bool) -> ModbusResult<()> {
        self.validate(RegisterType::DiscreteInput, address, 1)?;

        let mut inputs = self.discrete_inputs.write();
        inputs[address as usize] = value;
        Ok(())
    }

    // === Holding Register operations ===

    /// Read holding registers.
    pub fn read_holding_registers(&self, address: u16, quantity: u16) -> ModbusResult<Vec<u16>> {
        self.validate(RegisterType::HoldingRegister, address, quantity)?;

        let registers = self.holding_registers.read();
        let start = address as usize;
        let end = start + quantity as usize;
        Ok(registers[start..end].to_vec())
    }

    /// Write a single holding register.
    pub fn write_holding_register(&self, address: u16, value: u16) -> ModbusResult<()> {
        self.validate(RegisterType::HoldingRegister, address, 1)?;

        let mut registers = self.holding_registers.write();
        registers[address as usize] = value;
        Ok(())
    }

    /// Write multiple holding registers.
    pub fn write_holding_registers(&self, address: u16, values: &[u16]) -> ModbusResult<()> {
        self.validate(RegisterType::HoldingRegister, address, values.len() as u16)?;

        let mut registers = self.holding_registers.write();
        for (i, &value) in values.iter().enumerate() {
            registers[address as usize + i] = value;
        }
        Ok(())
    }

    // === Input Register operations ===

    /// Read input registers.
    pub fn read_input_registers(&self, address: u16, quantity: u16) -> ModbusResult<Vec<u16>> {
        self.validate(RegisterType::InputRegister, address, quantity)?;

        let registers = self.input_registers.read();
        let start = address as usize;
        let end = start + quantity as usize;
        Ok(registers[start..end].to_vec())
    }

    /// Set input register (internal use for simulation).
    pub fn set_input_register(&self, address: u16, value: u16) -> ModbusResult<()> {
        self.validate(RegisterType::InputRegister, address, 1)?;

        let mut registers = self.input_registers.write();
        registers[address as usize] = value;
        Ok(())
    }

    /// Set multiple input registers (internal use for simulation).
    pub fn set_input_registers(&self, address: u16, values: &[u16]) -> ModbusResult<()> {
        self.validate(RegisterType::InputRegister, address, values.len() as u16)?;

        let mut registers = self.input_registers.write();
        for (i, &value) in values.iter().enumerate() {
            registers[address as usize + i] = value;
        }
        Ok(())
    }

    // === Generic operations ===

    /// Read raw bytes from registers (for f32, f64, etc.).
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
            _ => return Err(ModbusError::InvalidFunction(reg_type.read_function())),
        };

        let mut bytes = Vec::with_capacity(byte_count);
        for reg in registers {
            bytes.extend_from_slice(&reg.to_be_bytes());
        }
        bytes.truncate(byte_count);
        Ok(bytes)
    }

    /// Write bytes to registers (for f32, f64, etc.).
    pub fn write_bytes(
        &self,
        reg_type: RegisterType,
        address: u16,
        bytes: &[u8],
    ) -> ModbusResult<()> {
        if !reg_type.is_writable() || reg_type.is_bit_type() {
            return Err(ModbusError::InvalidFunction(0));
        }

        let mut registers = Vec::new();
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

    /// Reset all registers to zero/false.
    pub fn reset(&self) {
        self.coils.write().fill(false);
        self.discrete_inputs.write().fill(false);
        self.holding_registers.write().fill(0);
        self.input_registers.write().fill(0);
    }
}

impl Default for RegisterStore {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_store_creation() {
        let store = RegisterStore::new(100, 100, 100, 100);
        assert_eq!(store.count(RegisterType::Coil), 100);
        assert_eq!(store.count(RegisterType::HoldingRegister), 100);
    }

    #[test]
    fn test_coil_operations() {
        let store = RegisterStore::with_defaults();

        // Write and read single coil
        store.write_coil(0, true).unwrap();
        let values = store.read_coils(0, 1).unwrap();
        assert_eq!(values, vec![true]);

        // Write and read multiple coils
        store.write_coils(10, &[true, false, true]).unwrap();
        let values = store.read_coils(10, 3).unwrap();
        assert_eq!(values, vec![true, false, true]);
    }

    #[test]
    fn test_holding_register_operations() {
        let store = RegisterStore::with_defaults();

        // Write and read single register
        store.write_holding_register(0, 12345).unwrap();
        let values = store.read_holding_registers(0, 1).unwrap();
        assert_eq!(values, vec![12345]);

        // Write and read multiple registers
        store.write_holding_registers(10, &[100, 200, 300]).unwrap();
        let values = store.read_holding_registers(10, 3).unwrap();
        assert_eq!(values, vec![100, 200, 300]);
    }

    #[test]
    fn test_invalid_address() {
        let store = RegisterStore::new(100, 100, 100, 100);

        // Address out of range
        let result = store.read_coils(100, 1);
        assert!(result.is_err());

        // Quantity out of range
        let result = store.read_coils(99, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_float_operations() {
        let store = RegisterStore::with_defaults();

        // Write f32 as bytes
        let f32_value: f32 = 3.14159;
        let bytes = f32_value.to_be_bytes();
        store
            .write_bytes(RegisterType::HoldingRegister, 0, &bytes)
            .unwrap();

        // Read back
        let read_bytes = store
            .read_bytes(RegisterType::HoldingRegister, 0, 4)
            .unwrap();
        let read_value =
            f32::from_be_bytes([read_bytes[0], read_bytes[1], read_bytes[2], read_bytes[3]]);
        assert!((read_value - f32_value).abs() < 0.0001);
    }
}
