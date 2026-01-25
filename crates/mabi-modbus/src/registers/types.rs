//! Register type definitions.

use serde::{Deserialize, Serialize};

/// Modbus register types.
///
/// Defines the four standard register types in the Modbus protocol,
/// each with its own address space and access characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegisterType {
    /// Coil (read/write, 1 bit).
    ///
    /// Modbus address range: 00001-09999 (protocol uses 0x0000-0x270E)
    /// Function codes: FC01 (read), FC05 (write single), FC15 (write multiple)
    Coil,

    /// Discrete input (read-only, 1 bit).
    ///
    /// Modbus address range: 10001-19999 (protocol uses 0x0000-0x270E)
    /// Function codes: FC02 (read)
    DiscreteInput,

    /// Holding register (read/write, 16 bits).
    ///
    /// Modbus address range: 40001-49999 (protocol uses 0x0000-0x270E)
    /// Function codes: FC03 (read), FC06 (write single), FC16 (write multiple)
    HoldingRegister,

    /// Input register (read-only, 16 bits).
    ///
    /// Modbus address range: 30001-39999 (protocol uses 0x0000-0x270E)
    /// Function codes: FC04 (read)
    InputRegister,
}

impl RegisterType {
    /// Get the Modbus function code for reading this register type.
    #[inline]
    pub const fn read_function_code(&self) -> u8 {
        match self {
            Self::Coil => 0x01,
            Self::DiscreteInput => 0x02,
            Self::HoldingRegister => 0x03,
            Self::InputRegister => 0x04,
        }
    }

    /// Get the Modbus function code for writing single value (None for read-only types).
    #[inline]
    pub const fn write_single_function_code(&self) -> Option<u8> {
        match self {
            Self::Coil => Some(0x05),
            Self::HoldingRegister => Some(0x06),
            Self::DiscreteInput | Self::InputRegister => None,
        }
    }

    /// Get the Modbus function code for writing multiple values (None for read-only types).
    #[inline]
    pub const fn write_multiple_function_code(&self) -> Option<u8> {
        match self {
            Self::Coil => Some(0x0F),
            Self::HoldingRegister => Some(0x10),
            Self::DiscreteInput | Self::InputRegister => None,
        }
    }

    /// Check if this register type is writable by external clients.
    #[inline]
    pub const fn is_writable(&self) -> bool {
        matches!(self, Self::Coil | Self::HoldingRegister)
    }

    /// Check if this register type is read-only for external clients.
    #[inline]
    pub const fn is_read_only(&self) -> bool {
        matches!(self, Self::DiscreteInput | Self::InputRegister)
    }

    /// Check if this is a bit type (coil or discrete input).
    #[inline]
    pub const fn is_bit_type(&self) -> bool {
        matches!(self, Self::Coil | Self::DiscreteInput)
    }

    /// Check if this is a word type (holding or input register).
    #[inline]
    pub const fn is_word_type(&self) -> bool {
        matches!(self, Self::HoldingRegister | Self::InputRegister)
    }

    /// Maximum quantity for read operation (per Modbus spec).
    #[inline]
    pub const fn max_read_quantity(&self) -> u16 {
        if self.is_bit_type() {
            2000 // Max bits per read
        } else {
            125 // Max registers per read
        }
    }

    /// Maximum quantity for write operation (per Modbus spec).
    #[inline]
    pub const fn max_write_quantity(&self) -> u16 {
        if self.is_bit_type() {
            1968 // Max bits per write
        } else {
            123 // Max registers per write
        }
    }

    /// Get the bit width of this register type.
    #[inline]
    pub const fn bit_width(&self) -> u8 {
        if self.is_bit_type() {
            1
        } else {
            16
        }
    }

    /// Get the byte size of a single register of this type.
    #[inline]
    pub const fn byte_size(&self) -> usize {
        if self.is_bit_type() {
            1 // Stored as bool but logically 1 bit
        } else {
            2 // u16
        }
    }

    /// Get the default Modbus address range for this type.
    #[inline]
    pub const fn default_address_range(&self) -> (u16, u16) {
        // Protocol-level addresses (0-based, 0x0000-0xFFFF)
        (0, 9999)
    }

    /// Get the human-readable name.
    #[inline]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Coil => "Coil",
            Self::DiscreteInput => "Discrete Input",
            Self::HoldingRegister => "Holding Register",
            Self::InputRegister => "Input Register",
        }
    }

    /// Get the abbreviated name.
    #[inline]
    pub const fn short_name(&self) -> &'static str {
        match self {
            Self::Coil => "CO",
            Self::DiscreteInput => "DI",
            Self::HoldingRegister => "HR",
            Self::InputRegister => "IR",
        }
    }

    /// Iterate over all register types.
    pub fn all() -> impl Iterator<Item = Self> {
        [
            Self::Coil,
            Self::DiscreteInput,
            Self::HoldingRegister,
            Self::InputRegister,
        ]
        .into_iter()
    }
}

impl std::fmt::Display for RegisterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_type_properties() {
        // Coils
        assert!(RegisterType::Coil.is_writable());
        assert!(RegisterType::Coil.is_bit_type());
        assert!(!RegisterType::Coil.is_word_type());
        assert_eq!(RegisterType::Coil.read_function_code(), 0x01);
        assert_eq!(RegisterType::Coil.write_single_function_code(), Some(0x05));

        // Discrete Inputs
        assert!(!RegisterType::DiscreteInput.is_writable());
        assert!(RegisterType::DiscreteInput.is_read_only());
        assert!(RegisterType::DiscreteInput.is_bit_type());
        assert_eq!(RegisterType::DiscreteInput.read_function_code(), 0x02);
        assert_eq!(RegisterType::DiscreteInput.write_single_function_code(), None);

        // Holding Registers
        assert!(RegisterType::HoldingRegister.is_writable());
        assert!(RegisterType::HoldingRegister.is_word_type());
        assert_eq!(RegisterType::HoldingRegister.read_function_code(), 0x03);
        assert_eq!(
            RegisterType::HoldingRegister.write_single_function_code(),
            Some(0x06)
        );

        // Input Registers
        assert!(!RegisterType::InputRegister.is_writable());
        assert!(RegisterType::InputRegister.is_read_only());
        assert!(RegisterType::InputRegister.is_word_type());
        assert_eq!(RegisterType::InputRegister.read_function_code(), 0x04);
        assert_eq!(RegisterType::InputRegister.write_single_function_code(), None);
    }

    #[test]
    fn test_max_quantities() {
        // Bit types
        assert_eq!(RegisterType::Coil.max_read_quantity(), 2000);
        assert_eq!(RegisterType::Coil.max_write_quantity(), 1968);
        assert_eq!(RegisterType::DiscreteInput.max_read_quantity(), 2000);

        // Word types
        assert_eq!(RegisterType::HoldingRegister.max_read_quantity(), 125);
        assert_eq!(RegisterType::HoldingRegister.max_write_quantity(), 123);
        assert_eq!(RegisterType::InputRegister.max_read_quantity(), 125);
    }

    #[test]
    fn test_all_iterator() {
        let types: Vec<_> = RegisterType::all().collect();
        assert_eq!(types.len(), 4);
        assert!(types.contains(&RegisterType::Coil));
        assert!(types.contains(&RegisterType::DiscreteInput));
        assert!(types.contains(&RegisterType::HoldingRegister));
        assert!(types.contains(&RegisterType::InputRegister));
    }
}
