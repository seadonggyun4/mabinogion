//! Read function code handlers.
//!
//! This module implements handlers for Modbus read operations:
//! - FC01: Read Coils
//! - FC02: Read Discrete Inputs
//! - FC03: Read Holding Registers
//! - FC04: Read Input Registers

use crate::register::RegisterType;

use super::{ExceptionCode, FunctionHandler, FunctionHandlerExt, HandlerContext};

/// Validates read quantity based on register type.
fn validate_read_quantity(quantity: u16, reg_type: RegisterType) -> Result<(), ExceptionCode> {
    if quantity == 0 {
        return Err(ExceptionCode::IllegalDataValue);
    }

    let max = reg_type.max_read_quantity();
    if quantity > max {
        return Err(ExceptionCode::IllegalDataValue);
    }

    Ok(())
}

/// FC01: Read Coils Handler
///
/// Reads the ON/OFF status of discrete coils in the server.
///
/// Request PDU:
/// - Function code: 1 byte (0x01)
/// - Starting address: 2 bytes
/// - Quantity of coils: 2 bytes (1-2000)
///
/// Response PDU:
/// - Function code: 1 byte (0x01)
/// - Byte count: 1 byte
/// - Coil status: N bytes (packed bits)
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadCoilsHandler;

impl FunctionHandler for ReadCoilsHandler {
    fn function_code(&self) -> u8 {
        0x01
    }

    fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
        self.validate_pdu_length(pdu)?;

        let address = self.parse_address(pdu, 1)?;
        let quantity = self.parse_quantity(pdu, 3)?;

        validate_read_quantity(quantity, RegisterType::Coil)?;

        // Read coils from register store
        let coils = ctx
            .registers
            .read_coils(address, quantity)
            .map_err(|_| ExceptionCode::IllegalDataAddress)?;

        // Pack bits into bytes
        let byte_count = quantity.div_ceil(8) as u8;
        let mut response = vec![self.function_code(), byte_count];
        let mut bytes = vec![0u8; byte_count as usize];

        for (i, &coil) in coils.iter().enumerate() {
            if coil {
                bytes[i / 8] |= 1 << (i % 8);
            }
        }

        response.extend_from_slice(&bytes);
        Ok(response)
    }

    fn name(&self) -> &'static str {
        "Read Coils"
    }

    fn min_pdu_length(&self) -> usize {
        5 // FC + address(2) + quantity(2)
    }
}

/// FC02: Read Discrete Inputs Handler
///
/// Reads the ON/OFF status of discrete inputs in the server.
///
/// Request PDU:
/// - Function code: 1 byte (0x02)
/// - Starting address: 2 bytes
/// - Quantity of inputs: 2 bytes (1-2000)
///
/// Response PDU:
/// - Function code: 1 byte (0x02)
/// - Byte count: 1 byte
/// - Input status: N bytes (packed bits)
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadDiscreteInputsHandler;

impl FunctionHandler for ReadDiscreteInputsHandler {
    fn function_code(&self) -> u8 {
        0x02
    }

    fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
        self.validate_pdu_length(pdu)?;

        let address = self.parse_address(pdu, 1)?;
        let quantity = self.parse_quantity(pdu, 3)?;

        validate_read_quantity(quantity, RegisterType::DiscreteInput)?;

        // Read discrete inputs from register store
        let inputs = ctx
            .registers
            .read_discrete_inputs(address, quantity)
            .map_err(|_| ExceptionCode::IllegalDataAddress)?;

        // Pack bits into bytes
        let byte_count = quantity.div_ceil(8) as u8;
        let mut response = vec![self.function_code(), byte_count];
        let mut bytes = vec![0u8; byte_count as usize];

        for (i, &input) in inputs.iter().enumerate() {
            if input {
                bytes[i / 8] |= 1 << (i % 8);
            }
        }

        response.extend_from_slice(&bytes);
        Ok(response)
    }

    fn name(&self) -> &'static str {
        "Read Discrete Inputs"
    }

    fn min_pdu_length(&self) -> usize {
        5 // FC + address(2) + quantity(2)
    }
}

/// FC03: Read Holding Registers Handler
///
/// Reads the contents of contiguous holding registers.
///
/// Request PDU:
/// - Function code: 1 byte (0x03)
/// - Starting address: 2 bytes
/// - Quantity of registers: 2 bytes (1-125)
///
/// Response PDU:
/// - Function code: 1 byte (0x03)
/// - Byte count: 1 byte
/// - Register values: N*2 bytes
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadHoldingRegistersHandler;

impl FunctionHandler for ReadHoldingRegistersHandler {
    fn function_code(&self) -> u8 {
        0x03
    }

    fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
        self.validate_pdu_length(pdu)?;

        let address = self.parse_address(pdu, 1)?;
        let quantity = self.parse_quantity(pdu, 3)?;

        validate_read_quantity(quantity, RegisterType::HoldingRegister)?;

        // Read holding registers
        let values = ctx
            .registers
            .read_holding_registers(address, quantity)
            .map_err(|_| ExceptionCode::IllegalDataAddress)?;

        // Build response
        let byte_count = (quantity * 2) as u8;
        let mut response = vec![self.function_code(), byte_count];

        for value in values {
            response.extend_from_slice(&value.to_be_bytes());
        }

        Ok(response)
    }

    fn name(&self) -> &'static str {
        "Read Holding Registers"
    }

    fn min_pdu_length(&self) -> usize {
        5 // FC + address(2) + quantity(2)
    }
}

/// FC04: Read Input Registers Handler
///
/// Reads the contents of contiguous input registers.
///
/// Request PDU:
/// - Function code: 1 byte (0x04)
/// - Starting address: 2 bytes
/// - Quantity of registers: 2 bytes (1-125)
///
/// Response PDU:
/// - Function code: 1 byte (0x04)
/// - Byte count: 1 byte
/// - Register values: N*2 bytes
#[derive(Debug, Clone, Copy, Default)]
pub struct ReadInputRegistersHandler;

impl FunctionHandler for ReadInputRegistersHandler {
    fn function_code(&self) -> u8 {
        0x04
    }

    fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
        self.validate_pdu_length(pdu)?;

        let address = self.parse_address(pdu, 1)?;
        let quantity = self.parse_quantity(pdu, 3)?;

        validate_read_quantity(quantity, RegisterType::InputRegister)?;

        // Read input registers
        let values = ctx
            .registers
            .read_input_registers(address, quantity)
            .map_err(|_| ExceptionCode::IllegalDataAddress)?;

        // Build response
        let byte_count = (quantity * 2) as u8;
        let mut response = vec![self.function_code(), byte_count];

        for value in values {
            response.extend_from_slice(&value.to_be_bytes());
        }

        Ok(response)
    }

    fn name(&self) -> &'static str {
        "Read Input Registers"
    }

    fn min_pdu_length(&self) -> usize {
        5 // FC + address(2) + quantity(2)
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
    fn test_read_coils() {
        let handler = ReadCoilsHandler;
        let ctx = create_context();

        // Set some coils
        ctx.registers.write_coils(0, &[true, false, true, true]).unwrap();

        // Read 4 coils from address 0
        let pdu = [0x01, 0x00, 0x00, 0x00, 0x04];
        let response = handler.handle(&pdu, &ctx).unwrap();

        assert_eq!(response[0], 0x01); // Function code
        assert_eq!(response[1], 1);    // Byte count
        assert_eq!(response[2], 0b00001101); // Coils: 1,0,1,1 = 0x0D
    }

    #[test]
    fn test_read_coils_invalid_quantity() {
        let handler = ReadCoilsHandler;
        let ctx = create_context();

        // Request 0 coils (invalid)
        let pdu = [0x01, 0x00, 0x00, 0x00, 0x00];
        let result = handler.handle(&pdu, &ctx);

        assert_eq!(result, Err(ExceptionCode::IllegalDataValue));
    }

    #[test]
    fn test_read_holding_registers() {
        let handler = ReadHoldingRegistersHandler;
        let ctx = create_context();

        // Set some registers
        ctx.registers.write_holding_registers(0, &[100, 200, 300]).unwrap();

        // Read 3 registers from address 0
        let pdu = [0x03, 0x00, 0x00, 0x00, 0x03];
        let response = handler.handle(&pdu, &ctx).unwrap();

        assert_eq!(response[0], 0x03); // Function code
        assert_eq!(response[1], 6);    // Byte count (3 regs * 2 bytes)
        assert_eq!(u16::from_be_bytes([response[2], response[3]]), 100);
        assert_eq!(u16::from_be_bytes([response[4], response[5]]), 200);
        assert_eq!(u16::from_be_bytes([response[6], response[7]]), 300);
    }

    #[test]
    fn test_read_holding_registers_invalid_address() {
        let handler = ReadHoldingRegistersHandler;
        let ctx = create_context();

        // Read from invalid address (beyond register count)
        let pdu = [0x03, 0xFF, 0xFF, 0x00, 0x01];
        let result = handler.handle(&pdu, &ctx);

        assert_eq!(result, Err(ExceptionCode::IllegalDataAddress));
    }

    #[test]
    fn test_read_input_registers() {
        let handler = ReadInputRegistersHandler;
        let ctx = create_context();

        // Set some input registers
        ctx.registers.set_input_register(10, 1234).unwrap();
        ctx.registers.set_input_register(11, 5678).unwrap();

        // Read 2 registers from address 10
        let pdu = [0x04, 0x00, 0x0A, 0x00, 0x02];
        let response = handler.handle(&pdu, &ctx).unwrap();

        assert_eq!(response[0], 0x04); // Function code
        assert_eq!(response[1], 4);    // Byte count
        assert_eq!(u16::from_be_bytes([response[2], response[3]]), 1234);
        assert_eq!(u16::from_be_bytes([response[4], response[5]]), 5678);
    }

    #[test]
    fn test_pdu_too_short() {
        let handler = ReadHoldingRegistersHandler;
        let ctx = create_context();

        // PDU too short (only 4 bytes instead of 5)
        let pdu = [0x03, 0x00, 0x00, 0x00];
        let result = handler.handle(&pdu, &ctx);

        assert_eq!(result, Err(ExceptionCode::IllegalDataValue));
    }
}
