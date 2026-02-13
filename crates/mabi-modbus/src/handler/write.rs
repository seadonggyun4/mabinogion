//! Write function code handlers.
//!
//! This module implements handlers for Modbus write operations:
//! - FC05: Write Single Coil
//! - FC06: Write Single Register
//! - FC0F: Write Multiple Coils
//! - FC10: Write Multiple Registers
//! - FC17: Read/Write Multiple Registers

use crate::register::RegisterType;

use super::{ExceptionCode, FunctionHandler, FunctionHandlerExt, HandlerContext};

/// Validates write quantity based on register type.
fn validate_write_quantity(quantity: u16, reg_type: RegisterType) -> Result<(), ExceptionCode> {
    if quantity == 0 {
        return Err(ExceptionCode::IllegalDataValue);
    }

    let max = reg_type.max_write_quantity();
    if quantity > max {
        return Err(ExceptionCode::IllegalDataValue);
    }

    Ok(())
}

/// FC05: Write Single Coil Handler
///
/// Forces a single coil to either ON or OFF.
///
/// Request PDU:
/// - Function code: 1 byte (0x05)
/// - Output address: 2 bytes
/// - Output value: 2 bytes (0x0000 = OFF, 0xFF00 = ON)
///
/// Response PDU (echo of request):
/// - Function code: 1 byte (0x05)
/// - Output address: 2 bytes
/// - Output value: 2 bytes
#[derive(Debug, Clone, Copy, Default)]
pub struct WriteSingleCoilHandler;

impl FunctionHandler for WriteSingleCoilHandler {
    fn function_code(&self) -> u8 {
        0x05
    }

    fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
        self.validate_pdu_length(pdu)?;

        let address = self.parse_address(pdu, 1)?;
        let value = u16::from_be_bytes([pdu[3], pdu[4]]);

        // Validate value (only 0x0000 or 0xFF00 allowed)
        if value != 0x0000 && value != 0xFF00 {
            return Err(ExceptionCode::IllegalDataValue);
        }

        let coil_value = value == 0xFF00;

        // Write coil
        ctx.registers
            .write_coil(address, coil_value)
            .map_err(|_| ExceptionCode::IllegalDataAddress)?;

        // Echo request as response
        Ok(pdu.to_vec())
    }

    fn name(&self) -> &'static str {
        "Write Single Coil"
    }

    fn min_pdu_length(&self) -> usize {
        5 // FC + address(2) + value(2)
    }

    fn supports_broadcast(&self) -> bool {
        true
    }
}

/// FC06: Write Single Register Handler
///
/// Writes a value into a single holding register.
///
/// Request PDU:
/// - Function code: 1 byte (0x06)
/// - Register address: 2 bytes
/// - Register value: 2 bytes
///
/// Response PDU (echo of request):
/// - Function code: 1 byte (0x06)
/// - Register address: 2 bytes
/// - Register value: 2 bytes
#[derive(Debug, Clone, Copy, Default)]
pub struct WriteSingleRegisterHandler;

impl FunctionHandler for WriteSingleRegisterHandler {
    fn function_code(&self) -> u8 {
        0x06
    }

    fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
        self.validate_pdu_length(pdu)?;

        let address = self.parse_address(pdu, 1)?;
        let value = u16::from_be_bytes([pdu[3], pdu[4]]);

        // Write register
        ctx.registers
            .write_holding_register(address, value)
            .map_err(|_| ExceptionCode::IllegalDataAddress)?;

        // Echo request as response
        Ok(pdu.to_vec())
    }

    fn name(&self) -> &'static str {
        "Write Single Register"
    }

    fn min_pdu_length(&self) -> usize {
        5 // FC + address(2) + value(2)
    }

    fn supports_broadcast(&self) -> bool {
        true
    }
}

/// FC0F: Write Multiple Coils Handler
///
/// Forces each coil in a sequence of coils to either ON or OFF.
///
/// Request PDU:
/// - Function code: 1 byte (0x0F)
/// - Starting address: 2 bytes
/// - Quantity of outputs: 2 bytes (1-1968)
/// - Byte count: 1 byte
/// - Output values: N bytes (packed bits)
///
/// Response PDU:
/// - Function code: 1 byte (0x0F)
/// - Starting address: 2 bytes
/// - Quantity of outputs: 2 bytes
#[derive(Debug, Clone, Copy, Default)]
pub struct WriteMultipleCoilsHandler;

impl FunctionHandler for WriteMultipleCoilsHandler {
    fn function_code(&self) -> u8 {
        0x0F
    }

    fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
        self.validate_pdu_length(pdu)?;

        let address = self.parse_address(pdu, 1)?;
        let quantity = self.parse_quantity(pdu, 3)?;

        validate_write_quantity(quantity, RegisterType::Coil)?;

        // Validate byte count
        let expected_bytes = quantity.div_ceil(8) as usize;
        let byte_count = pdu[5] as usize;

        if byte_count != expected_bytes {
            return Err(ExceptionCode::IllegalDataValue);
        }

        // Check if we have enough data
        if pdu.len() < 6 + byte_count {
            return Err(ExceptionCode::IllegalDataValue);
        }

        // Unpack coil values from bytes
        let data = &pdu[6..6 + byte_count];
        let mut coils = Vec::with_capacity(quantity as usize);

        for i in 0..quantity as usize {
            coils.push((data[i / 8] & (1 << (i % 8))) != 0);
        }

        // Write coils
        ctx.registers
            .write_coils(address, &coils)
            .map_err(|_| ExceptionCode::IllegalDataAddress)?;

        // Build response
        let mut response = vec![self.function_code()];
        response.extend_from_slice(&address.to_be_bytes());
        response.extend_from_slice(&quantity.to_be_bytes());

        Ok(response)
    }

    fn name(&self) -> &'static str {
        "Write Multiple Coils"
    }

    fn min_pdu_length(&self) -> usize {
        6 // FC + address(2) + quantity(2) + byte_count(1)
    }

    fn supports_broadcast(&self) -> bool {
        true
    }
}

/// FC10: Write Multiple Registers Handler
///
/// Writes values into a sequence of holding registers.
///
/// Request PDU:
/// - Function code: 1 byte (0x10)
/// - Starting address: 2 bytes
/// - Quantity of registers: 2 bytes (1-123)
/// - Byte count: 1 byte
/// - Register values: N*2 bytes
///
/// Response PDU:
/// - Function code: 1 byte (0x10)
/// - Starting address: 2 bytes
/// - Quantity of registers: 2 bytes
#[derive(Debug, Clone, Copy, Default)]
pub struct WriteMultipleRegistersHandler;

impl FunctionHandler for WriteMultipleRegistersHandler {
    fn function_code(&self) -> u8 {
        0x10
    }

    fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
        self.validate_pdu_length(pdu)?;

        let address = self.parse_address(pdu, 1)?;
        let quantity = self.parse_quantity(pdu, 3)?;

        validate_write_quantity(quantity, RegisterType::HoldingRegister)?;

        // Validate byte count
        let expected_bytes = (quantity * 2) as usize;
        let byte_count = pdu[5] as usize;

        if byte_count != expected_bytes {
            return Err(ExceptionCode::IllegalDataValue);
        }

        // Check if we have enough data
        if pdu.len() < 6 + byte_count {
            return Err(ExceptionCode::IllegalDataValue);
        }

        // Parse register values
        let data = &pdu[6..6 + byte_count];
        let mut values = Vec::with_capacity(quantity as usize);

        for i in 0..quantity as usize {
            values.push(u16::from_be_bytes([data[i * 2], data[i * 2 + 1]]));
        }

        // Write registers
        ctx.registers
            .write_holding_registers(address, &values)
            .map_err(|_| ExceptionCode::IllegalDataAddress)?;

        // Build response
        let mut response = vec![self.function_code()];
        response.extend_from_slice(&address.to_be_bytes());
        response.extend_from_slice(&quantity.to_be_bytes());

        Ok(response)
    }

    fn name(&self) -> &'static str {
        "Write Multiple Registers"
    }

    fn min_pdu_length(&self) -> usize {
        6 // FC + address(2) + quantity(2) + byte_count(1)
    }

    fn supports_broadcast(&self) -> bool {
        true
    }
}

/// FC17: Read/Write Multiple Registers Handler
///
/// Performs a combination of one read operation and one write operation
/// in a single Modbus transaction.
///
/// Request PDU:
/// - Function code: 1 byte (0x17)
/// - Read starting address: 2 bytes
/// - Quantity to read: 2 bytes (1-125)
/// - Write starting address: 2 bytes
/// - Quantity to write: 2 bytes (1-121)
/// - Write byte count: 1 byte
/// - Write register values: N*2 bytes
///
/// Response PDU:
/// - Function code: 1 byte (0x17)
/// - Byte count: 1 byte
/// - Read register values: N*2 bytes
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadWriteMultipleRegistersHandler;

impl FunctionHandler for ReadWriteMultipleRegistersHandler {
    fn function_code(&self) -> u8 {
        0x17
    }

    fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
        self.validate_pdu_length(pdu)?;

        // Parse request
        let read_address = self.parse_address(pdu, 1)?;
        let read_quantity = self.parse_quantity(pdu, 3)?;
        let write_address = self.parse_address(pdu, 5)?;
        let write_quantity = self.parse_quantity(pdu, 7)?;

        // Validate quantities
        if read_quantity == 0 || read_quantity > 125 {
            return Err(ExceptionCode::IllegalDataValue);
        }
        if write_quantity == 0 || write_quantity > 121 {
            return Err(ExceptionCode::IllegalDataValue);
        }

        // Validate byte count
        let expected_bytes = (write_quantity * 2) as usize;
        let byte_count = pdu[9] as usize;

        if byte_count != expected_bytes {
            return Err(ExceptionCode::IllegalDataValue);
        }

        // Check if we have enough data
        if pdu.len() < 10 + byte_count {
            return Err(ExceptionCode::IllegalDataValue);
        }

        // Parse write values
        let data = &pdu[10..10 + byte_count];
        let mut write_values = Vec::with_capacity(write_quantity as usize);

        for i in 0..write_quantity as usize {
            write_values.push(u16::from_be_bytes([data[i * 2], data[i * 2 + 1]]));
        }

        // Perform write first
        ctx.registers
            .write_holding_registers(write_address, &write_values)
            .map_err(|_| ExceptionCode::IllegalDataAddress)?;

        // Then read
        let read_values = ctx
            .registers
            .read_holding_registers(read_address, read_quantity)
            .map_err(|_| ExceptionCode::IllegalDataAddress)?;

        // Build response
        let response_byte_count = (read_quantity * 2) as u8;
        let mut response = vec![self.function_code(), response_byte_count];

        for value in read_values {
            response.extend_from_slice(&value.to_be_bytes());
        }

        Ok(response)
    }

    fn name(&self) -> &'static str {
        "Read/Write Multiple Registers"
    }

    fn min_pdu_length(&self) -> usize {
        10 // FC + read_addr(2) + read_qty(2) + write_addr(2) + write_qty(2) + byte_count(1)
    }
}

/// FC16 (0x16): Mask Write Register Handler
///
/// Modifies the contents of a specified holding register using a
/// combination of an AND mask, an OR mask, and the register's
/// current contents.
///
/// Formula: Result = (Current AND And_Mask) OR (Or_Mask AND NOT(And_Mask))
///
/// Request PDU:
/// - Function code: 1 byte (0x16)
/// - Reference address: 2 bytes
/// - And_Mask: 2 bytes
/// - Or_Mask: 2 bytes
///
/// Response PDU (echo of request):
/// - Function code: 1 byte (0x16)
/// - Reference address: 2 bytes
/// - And_Mask: 2 bytes
/// - Or_Mask: 2 bytes
#[derive(Debug, Clone, Copy, Default)]
pub struct MaskWriteRegisterHandler;

impl FunctionHandler for MaskWriteRegisterHandler {
    fn function_code(&self) -> u8 {
        0x16
    }

    fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
        self.validate_pdu_length(pdu)?;

        let address = self.parse_address(pdu, 1)?;
        let and_mask = u16::from_be_bytes([pdu[3], pdu[4]]);
        let or_mask = u16::from_be_bytes([pdu[5], pdu[6]]);

        // Perform atomic mask write (single lock acquisition)
        ctx.registers
            .mask_write_holding_register(address, and_mask, or_mask)
            .map_err(|_| ExceptionCode::IllegalDataAddress)?;

        // Echo request as response
        Ok(pdu.to_vec())
    }

    fn name(&self) -> &'static str {
        "Mask Write Register"
    }

    fn min_pdu_length(&self) -> usize {
        7 // FC + address(2) + and_mask(2) + or_mask(2)
    }

    fn supports_broadcast(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::register::RegisterStore;
    use std::sync::Arc;

    fn create_context() -> HandlerContext {
        let registers = Arc::new(RegisterStore::with_defaults());
        HandlerContext::new(1, registers, 1)
    }

    #[test]
    fn test_write_single_coil_on() {
        let handler = WriteSingleCoilHandler;
        let ctx = create_context();

        // Write ON (0xFF00) to address 5
        let pdu = [0x05, 0x00, 0x05, 0xFF, 0x00];
        let response = handler.handle(&pdu, &ctx).unwrap();

        // Response should echo request
        assert_eq!(response, pdu.to_vec());

        // Verify coil was set
        let coils = ctx.registers.read_coils(5, 1).unwrap();
        assert!(coils[0]);
    }

    #[test]
    fn test_write_single_coil_off() {
        let handler = WriteSingleCoilHandler;
        let ctx = create_context();

        // First set the coil
        ctx.registers.write_coil(5, true).unwrap();

        // Write OFF (0x0000) to address 5
        let pdu = [0x05, 0x00, 0x05, 0x00, 0x00];
        let response = handler.handle(&pdu, &ctx).unwrap();

        assert_eq!(response, pdu.to_vec());

        // Verify coil was cleared
        let coils = ctx.registers.read_coils(5, 1).unwrap();
        assert!(!coils[0]);
    }

    #[test]
    fn test_write_single_coil_invalid_value() {
        let handler = WriteSingleCoilHandler;
        let ctx = create_context();

        // Invalid value (not 0x0000 or 0xFF00)
        let pdu = [0x05, 0x00, 0x05, 0x12, 0x34];
        let result = handler.handle(&pdu, &ctx);

        assert_eq!(result, Err(ExceptionCode::IllegalDataValue));
    }

    #[test]
    fn test_write_single_register() {
        let handler = WriteSingleRegisterHandler;
        let ctx = create_context();

        // Write 0x1234 to address 10
        let pdu = [0x06, 0x00, 0x0A, 0x12, 0x34];
        let response = handler.handle(&pdu, &ctx).unwrap();

        // Response should echo request
        assert_eq!(response, pdu.to_vec());

        // Verify register was written
        let values = ctx.registers.read_holding_registers(10, 1).unwrap();
        assert_eq!(values[0], 0x1234);
    }

    #[test]
    fn test_write_multiple_coils() {
        let handler = WriteMultipleCoilsHandler;
        let ctx = create_context();

        // Write 10 coils starting at address 0
        // Values: 1,0,1,1,0,1,0,1,1,1 = 0xAD 0x03
        let pdu = [0x0F, 0x00, 0x00, 0x00, 0x0A, 0x02, 0xAD, 0x03];
        let response = handler.handle(&pdu, &ctx).unwrap();

        // Check response
        assert_eq!(response[0], 0x0F);
        assert_eq!(u16::from_be_bytes([response[1], response[2]]), 0);
        assert_eq!(u16::from_be_bytes([response[3], response[4]]), 10);

        // Verify coils
        let coils = ctx.registers.read_coils(0, 10).unwrap();
        assert_eq!(coils, vec![true, false, true, true, false, true, false, true, true, true]);
    }

    #[test]
    fn test_write_multiple_registers() {
        let handler = WriteMultipleRegistersHandler;
        let ctx = create_context();

        // Write 3 registers starting at address 5
        let pdu = [
            0x10, 0x00, 0x05, // FC, address
            0x00, 0x03,       // quantity
            0x06,             // byte count
            0x00, 0x64,       // 100
            0x00, 0xC8,       // 200
            0x01, 0x2C,       // 300
        ];
        let response = handler.handle(&pdu, &ctx).unwrap();

        // Check response
        assert_eq!(response[0], 0x10);
        assert_eq!(u16::from_be_bytes([response[1], response[2]]), 5);
        assert_eq!(u16::from_be_bytes([response[3], response[4]]), 3);

        // Verify registers
        let values = ctx.registers.read_holding_registers(5, 3).unwrap();
        assert_eq!(values, vec![100, 200, 300]);
    }

    #[test]
    fn test_read_write_multiple_registers() {
        let handler = ReadWriteMultipleRegistersHandler;
        let ctx = create_context();

        // Pre-set some values to read
        ctx.registers.write_holding_registers(0, &[10, 20, 30]).unwrap();

        // Read 3 registers from addr 0, write 2 registers to addr 10
        let pdu = [
            0x17,
            0x00, 0x00,       // read address
            0x00, 0x03,       // read quantity
            0x00, 0x0A,       // write address
            0x00, 0x02,       // write quantity
            0x04,             // byte count
            0x01, 0x00,       // 256
            0x02, 0x00,       // 512
        ];
        let response = handler.handle(&pdu, &ctx).unwrap();

        // Check response contains read values
        assert_eq!(response[0], 0x17);
        assert_eq!(response[1], 6); // byte count
        assert_eq!(u16::from_be_bytes([response[2], response[3]]), 10);
        assert_eq!(u16::from_be_bytes([response[4], response[5]]), 20);
        assert_eq!(u16::from_be_bytes([response[6], response[7]]), 30);

        // Verify write was performed
        let values = ctx.registers.read_holding_registers(10, 2).unwrap();
        assert_eq!(values, vec![256, 512]);
    }

    #[test]
    fn test_mask_write_register_basic() {
        let handler = MaskWriteRegisterHandler;
        let ctx = create_context();

        // Set register 10 to 0x00FF
        ctx.registers.write_holding_register(10, 0x00FF).unwrap();

        // Apply And_Mask=0xFF00, Or_Mask=0x00F0
        // Result = (0x00FF & 0xFF00) | (0x00F0 & !0xFF00)
        //        = (0x0000)          | (0x00F0 & 0x00FF)
        //        = 0x0000            | 0x00F0
        //        = 0x00F0
        let pdu = [0x16, 0x00, 0x0A, 0xFF, 0x00, 0x00, 0xF0];
        let response = handler.handle(&pdu, &ctx).unwrap();

        // Response echoes request
        assert_eq!(response, pdu.to_vec());

        // Verify register value
        let values = ctx.registers.read_holding_registers(10, 1).unwrap();
        assert_eq!(values[0], 0x00F0);
    }

    #[test]
    fn test_mask_write_register_identity() {
        let handler = MaskWriteRegisterHandler;
        let ctx = create_context();

        // Set register 5 to 0xABCD
        ctx.registers.write_holding_register(5, 0xABCD).unwrap();

        // And_Mask=0xFFFF, Or_Mask=0x0000 => identity (no change)
        // Result = (0xABCD & 0xFFFF) | (0x0000 & !0xFFFF)
        //        = 0xABCD            | 0x0000
        //        = 0xABCD
        let pdu = [0x16, 0x00, 0x05, 0xFF, 0xFF, 0x00, 0x00];
        handler.handle(&pdu, &ctx).unwrap();

        let values = ctx.registers.read_holding_registers(5, 1).unwrap();
        assert_eq!(values[0], 0xABCD);
    }

    #[test]
    fn test_mask_write_register_force_all_bits() {
        let handler = MaskWriteRegisterHandler;
        let ctx = create_context();

        // Set register 0 to 0x1234
        ctx.registers.write_holding_register(0, 0x1234).unwrap();

        // And_Mask=0x0000, Or_Mask=0xABCD => force to Or_Mask value
        // Result = (0x1234 & 0x0000) | (0xABCD & !0x0000)
        //        = 0x0000            | (0xABCD & 0xFFFF)
        //        = 0xABCD
        let pdu = [0x16, 0x00, 0x00, 0x00, 0x00, 0xAB, 0xCD];
        handler.handle(&pdu, &ctx).unwrap();

        let values = ctx.registers.read_holding_registers(0, 1).unwrap();
        assert_eq!(values[0], 0xABCD);
    }

    #[test]
    fn test_mask_write_register_clear_low_byte() {
        let handler = MaskWriteRegisterHandler;
        let ctx = create_context();

        // Set register 0 to 0xABCD
        ctx.registers.write_holding_register(0, 0xABCD).unwrap();

        // And_Mask=0xFF00, Or_Mask=0x0000 => clear low byte
        // Result = (0xABCD & 0xFF00) | (0x0000 & !0xFF00)
        //        = 0xAB00            | 0x0000
        //        = 0xAB00
        let pdu = [0x16, 0x00, 0x00, 0xFF, 0x00, 0x00, 0x00];
        handler.handle(&pdu, &ctx).unwrap();

        let values = ctx.registers.read_holding_registers(0, 1).unwrap();
        assert_eq!(values[0], 0xAB00);
    }

    #[test]
    fn test_mask_write_register_set_single_bit() {
        let handler = MaskWriteRegisterHandler;
        let ctx = create_context();

        // Set register 0 to 0x0000
        ctx.registers.write_holding_register(0, 0x0000).unwrap();

        // Set bit 3 (0x0008): And_Mask=0xFFFF, Or_Mask=0x0008
        // Result = (0x0000 & 0xFFFF) | (0x0008 & !0xFFFF)
        //        = 0x0000            | (0x0008 & 0x0000)
        //        = 0x0000
        // Wait - to SET a bit: And_Mask should preserve all, Or_Mask sets the bit
        // But NOT(0xFFFF) = 0x0000, so Or_Mask has no effect.
        // Correct approach: And_Mask=0xFFFF, Or_Mask=0x0008 won't work.
        // Instead: And_Mask = keep all existing, and also OR in the bit:
        // And_Mask=0xFFFF preserves all, Or_Mask=0x0008
        // Result = (0x0000 & 0xFFFF) | (0x0008 & 0x0000) = 0x0000 ... wrong
        //
        // Actually per Modbus spec: to set bit 3, use And_Mask=0xFFFF, Or_Mask=0x0008
        // But: (0x0000 & 0xFFFF) | (0x0008 & ~0xFFFF) = 0 | (0x0008 & 0x0000) = 0
        // This is correct per spec - And_Mask=0xFFFF means "keep all original bits,
        // don't allow OR to change anything". To set bit 3:
        // And_Mask = 0xFFF7 (clear bit 3 position in and_mask so OR can write it)
        // Or_Mask = 0x0008
        // Result = (0x0000 & 0xFFF7) | (0x0008 & ~0xFFF7) = 0x0000 | (0x0008 & 0x0008) = 0x0008
        let pdu = [0x16, 0x00, 0x00, 0xFF, 0xF7, 0x00, 0x08];
        handler.handle(&pdu, &ctx).unwrap();

        let values = ctx.registers.read_holding_registers(0, 1).unwrap();
        assert_eq!(values[0], 0x0008);
    }

    #[test]
    fn test_mask_write_register_echo_response() {
        let handler = MaskWriteRegisterHandler;
        let ctx = create_context();

        let pdu = [0x16, 0x00, 0x04, 0x00, 0xF2, 0x00, 0x25];
        let response = handler.handle(&pdu, &ctx).unwrap();

        // Response must be exact echo of request
        assert_eq!(response, pdu.to_vec());
    }

    #[test]
    fn test_mask_write_register_pdu_too_short() {
        let handler = MaskWriteRegisterHandler;
        let ctx = create_context();

        // Only 6 bytes (need 7)
        let pdu = [0x16, 0x00, 0x00, 0xFF, 0xFF, 0x00];
        let result = handler.handle(&pdu, &ctx);

        assert_eq!(result, Err(ExceptionCode::IllegalDataValue));
    }

    #[test]
    fn test_write_multiple_registers_invalid_byte_count() {
        let handler = WriteMultipleRegistersHandler;
        let ctx = create_context();

        // Byte count doesn't match quantity
        let pdu = [
            0x10, 0x00, 0x05,
            0x00, 0x03,       // 3 registers = 6 bytes
            0x04,             // but byte count says 4
            0x00, 0x64,
            0x00, 0xC8,
        ];
        let result = handler.handle(&pdu, &ctx);

        assert_eq!(result, Err(ExceptionCode::IllegalDataValue));
    }
}
