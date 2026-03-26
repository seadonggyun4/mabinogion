//! Core traits for Modbus function handlers.

use crate::context::SharedAddressSpace;
use crate::error::ModbusResult;
use crate::types::{RegisterConverter, WordOrder};

use super::ExceptionCode;

/// Result type for handler operations.
pub type HandlerResult = ModbusResult<Vec<u8>>;

/// Context provided to handlers for processing requests.
///
/// This struct provides access to all resources needed by handlers
/// to process Modbus requests.
#[derive(Clone)]
pub struct HandlerContext {
    /// Unit ID from the request.
    pub unit_id: u8,

    /// Register store for this unit.
    pub registers: SharedAddressSpace,

    /// Transaction ID (for logging/tracing).
    pub transaction_id: u16,

    /// Word order for this unit (for multi-register value conversion).
    pub word_order: WordOrder,

    /// Value converter for this unit.
    pub converter: RegisterConverter,

    /// Whether this request is a broadcast (unit_id == 0).
    pub is_broadcast: bool,
}

impl HandlerContext {
    /// Create a new handler context.
    pub fn new(unit_id: u8, registers: SharedAddressSpace, transaction_id: u16) -> Self {
        let word_order = WordOrder::default();
        Self {
            unit_id,
            registers,
            transaction_id,
            word_order,
            converter: RegisterConverter::new(word_order),
            is_broadcast: unit_id == 0,
        }
    }

    /// Create a new handler context with specific word order.
    pub fn with_word_order(
        unit_id: u8,
        registers: SharedAddressSpace,
        transaction_id: u16,
        word_order: WordOrder,
    ) -> Self {
        Self {
            unit_id,
            registers,
            transaction_id,
            word_order,
            converter: RegisterConverter::new(word_order),
            is_broadcast: unit_id == 0,
        }
    }

    /// Create context for a broadcast request.
    pub fn broadcast(registers: SharedAddressSpace, transaction_id: u16) -> Self {
        Self::new(0, registers, transaction_id)
    }

    /// Get the value converter for this context.
    #[inline]
    pub fn converter(&self) -> &RegisterConverter {
        &self.converter
    }

    /// Check if this is a broadcast request.
    #[inline]
    pub fn is_broadcast(&self) -> bool {
        self.is_broadcast
    }
}

/// Trait for implementing Modbus function code handlers.
///
/// Each handler is responsible for:
/// 1. Parsing the PDU data for its function code
/// 2. Validating the request parameters
/// 3. Executing the operation on the register store
/// 4. Building the response PDU
///
/// # Implementation Guidelines
///
/// - Return `ExceptionCode` for protocol errors (invalid address, quantity, etc.)
/// - Use `HandlerResult` for internal errors
/// - Build response PDU starting with the function code
/// - Follow Modbus specification for PDU format
///
/// # Example
///
/// ```rust,ignore
/// use mabi_modbus::handler::{FunctionHandler, HandlerContext, HandlerResult, ExceptionCode};
///
/// pub struct MyCustomHandler;
///
/// impl FunctionHandler for MyCustomHandler {
///     fn function_code(&self) -> u8 {
///         0x42 // Custom function code
///     }
///
///     fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
///         // Validate PDU length
///         if pdu.len() < 5 {
///             return Err(ExceptionCode::IllegalDataValue);
///         }
///
///         // Parse request
///         let address = u16::from_be_bytes([pdu[1], pdu[2]]);
///         let value = u16::from_be_bytes([pdu[3], pdu[4]]);
///
///         // Execute operation
///         // ...
///
///         // Build response
///         let mut response = vec![self.function_code()];
///         response.extend_from_slice(&address.to_be_bytes());
///         response.extend_from_slice(&value.to_be_bytes());
///
///         Ok(response)
///     }
///
///     fn name(&self) -> &'static str {
///         "My Custom Handler"
///     }
/// }
/// ```
pub trait FunctionHandler: Send + Sync {
    /// Returns the Modbus function code this handler processes.
    fn function_code(&self) -> u8;

    /// Handle a Modbus request and return the response PDU.
    ///
    /// # Arguments
    ///
    /// * `pdu` - The Protocol Data Unit (function code + data)
    /// * `ctx` - Context containing registers and metadata
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<u8>)` - Response PDU (function code + response data)
    /// * `Err(ExceptionCode)` - Modbus exception to return to client
    fn handle(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode>;

    /// Returns a human-readable name for this handler.
    fn name(&self) -> &'static str;

    /// Returns the minimum PDU length required for this function.
    ///
    /// Override this to provide early validation before parsing.
    fn min_pdu_length(&self) -> usize {
        1 // At minimum, function code
    }

    /// Returns whether this handler supports broadcast (unit ID 0).
    ///
    /// By default, only write operations support broadcast.
    fn supports_broadcast(&self) -> bool {
        false
    }
}

/// Extension trait for function handlers that provides utility methods.
pub trait FunctionHandlerExt: FunctionHandler {
    /// Validate that the PDU has the minimum required length.
    fn validate_pdu_length(&self, pdu: &[u8]) -> Result<(), ExceptionCode> {
        if pdu.len() < self.min_pdu_length() {
            return Err(ExceptionCode::IllegalDataValue);
        }
        Ok(())
    }

    /// Parse a 16-bit address from the PDU at the given offset.
    fn parse_address(&self, pdu: &[u8], offset: usize) -> Result<u16, ExceptionCode> {
        if pdu.len() < offset + 2 {
            return Err(ExceptionCode::IllegalDataValue);
        }
        Ok(u16::from_be_bytes([pdu[offset], pdu[offset + 1]]))
    }

    /// Parse a 16-bit quantity from the PDU at the given offset.
    fn parse_quantity(&self, pdu: &[u8], offset: usize) -> Result<u16, ExceptionCode> {
        if pdu.len() < offset + 2 {
            return Err(ExceptionCode::IllegalDataValue);
        }
        Ok(u16::from_be_bytes([pdu[offset], pdu[offset + 1]]))
    }
}

// Automatically implement FunctionHandlerExt for all FunctionHandler types
impl<T: FunctionHandler + ?Sized> FunctionHandlerExt for T {}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHandler;

    impl FunctionHandler for TestHandler {
        fn function_code(&self) -> u8 {
            0x99
        }

        fn handle(&self, _pdu: &[u8], _ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
            Ok(vec![0x99])
        }

        fn name(&self) -> &'static str {
            "Test Handler"
        }

        fn min_pdu_length(&self) -> usize {
            5
        }
    }

    #[test]
    fn test_validate_pdu_length() {
        let handler = TestHandler;

        assert!(handler
            .validate_pdu_length(&[0x99, 0x00, 0x00, 0x00, 0x00])
            .is_ok());
        assert!(handler
            .validate_pdu_length(&[0x99, 0x00, 0x00, 0x00])
            .is_err());
    }

    #[test]
    fn test_parse_address() {
        let handler = TestHandler;
        let pdu = [0x99, 0x01, 0x00]; // FC, addr high, addr low

        assert_eq!(handler.parse_address(&pdu, 1).unwrap(), 0x0100);
    }
}
