//! Handler registry for dispatching Modbus requests.
//!
//! The `HandlerRegistry` provides a centralized mechanism for registering
//! and dispatching Modbus function handlers. It supports:
//!
//! - Default handlers for all standard function codes
//! - Custom handler registration
//! - Handler replacement and removal
//! - Introspection of registered handlers

use std::collections::HashMap;
use std::sync::Arc;

use tracing::{debug, warn};

use super::{
    ExceptionCode, FunctionHandler, HandlerContext, MaskWriteRegisterHandler,
    ReadCoilsHandler, ReadDiscreteInputsHandler, ReadHoldingRegistersHandler,
    ReadInputRegistersHandler, ReadWriteMultipleRegistersHandler, WriteMultipleCoilsHandler,
    WriteMultipleRegistersHandler, WriteSingleCoilHandler, WriteSingleRegisterHandler,
};

/// Registry for Modbus function handlers.
///
/// This struct manages the mapping between function codes and their handlers.
/// It supports dynamic registration and provides efficient dispatch.
///
/// # Thread Safety
///
/// The registry itself is not thread-safe. Wrap it in `Arc<RwLock<_>>` if you
/// need concurrent access with modifications. For read-only dispatch (the common case),
/// wrap in `Arc` and share between threads.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_modbus::handler::{HandlerRegistry, FunctionHandler};
///
/// // Create with default handlers
/// let mut registry = HandlerRegistry::with_defaults();
///
/// // Register a custom handler
/// registry.register(Box::new(MyCustomHandler));
///
/// // Dispatch a request
/// let response = registry.dispatch(function_code, &pdu, &context);
/// ```
pub struct HandlerRegistry {
    handlers: HashMap<u8, Arc<dyn FunctionHandler>>,
}

impl HandlerRegistry {
    /// Create an empty registry with no handlers.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Create a registry with all default Modbus handlers registered.
    ///
    /// This includes handlers for:
    /// - FC01: Read Coils
    /// - FC02: Read Discrete Inputs
    /// - FC03: Read Holding Registers
    /// - FC04: Read Input Registers
    /// - FC05: Write Single Coil
    /// - FC06: Write Single Register
    /// - FC0F: Write Multiple Coils
    /// - FC10: Write Multiple Registers
    /// - FC17: Read/Write Multiple Registers
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // Read handlers
        registry.register(Box::new(ReadCoilsHandler));
        registry.register(Box::new(ReadDiscreteInputsHandler));
        registry.register(Box::new(ReadHoldingRegistersHandler));
        registry.register(Box::new(ReadInputRegistersHandler));

        // Write handlers
        registry.register(Box::new(WriteSingleCoilHandler));
        registry.register(Box::new(WriteSingleRegisterHandler));
        registry.register(Box::new(WriteMultipleCoilsHandler));
        registry.register(Box::new(WriteMultipleRegistersHandler));
        registry.register(Box::new(ReadWriteMultipleRegistersHandler));
        registry.register(Box::new(MaskWriteRegisterHandler));

        registry
    }

    /// Register a handler for its function code.
    ///
    /// If a handler is already registered for this function code,
    /// it will be replaced.
    ///
    /// # Returns
    ///
    /// The previously registered handler, if any.
    pub fn register(
        &mut self,
        handler: Box<dyn FunctionHandler>,
    ) -> Option<Arc<dyn FunctionHandler>> {
        let fc = handler.function_code();
        debug!(function_code = fc, name = handler.name(), "Registering handler");
        self.handlers.insert(fc, Arc::from(handler))
    }

    /// Unregister a handler for the given function code.
    ///
    /// # Returns
    ///
    /// The removed handler, if one was registered.
    pub fn unregister(&mut self, function_code: u8) -> Option<Arc<dyn FunctionHandler>> {
        debug!(function_code, "Unregistering handler");
        self.handlers.remove(&function_code)
    }

    /// Get a handler for the given function code.
    pub fn get(&self, function_code: u8) -> Option<&Arc<dyn FunctionHandler>> {
        self.handlers.get(&function_code)
    }

    /// Check if a handler is registered for the given function code.
    pub fn has_handler(&self, function_code: u8) -> bool {
        self.handlers.contains_key(&function_code)
    }

    /// Get all registered function codes.
    pub fn function_codes(&self) -> Vec<u8> {
        self.handlers.keys().copied().collect()
    }

    /// Get information about all registered handlers.
    pub fn handler_info(&self) -> Vec<(u8, &'static str)> {
        self.handlers
            .iter()
            .map(|(&fc, h)| (fc, h.name()))
            .collect()
    }

    /// Dispatch a request to the appropriate handler.
    ///
    /// # Arguments
    ///
    /// * `pdu` - The Protocol Data Unit (must start with function code)
    /// * `ctx` - Handler context with registers and metadata
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<u8>)` - Response PDU
    /// * `Err(ExceptionCode)` - Modbus exception
    pub fn dispatch(&self, pdu: &[u8], ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
        if pdu.is_empty() {
            warn!("Empty PDU received");
            return Err(ExceptionCode::IllegalFunction);
        }

        let function_code = pdu[0];

        match self.handlers.get(&function_code) {
            Some(handler) => {
                // Check broadcast support
                if ctx.unit_id == 0 && !handler.supports_broadcast() {
                    warn!(
                        function_code,
                        "Broadcast not supported for this function"
                    );
                    return Err(ExceptionCode::IllegalFunction);
                }

                // Validate minimum PDU length
                if pdu.len() < handler.min_pdu_length() {
                    warn!(
                        function_code,
                        pdu_len = pdu.len(),
                        min_len = handler.min_pdu_length(),
                        "PDU too short"
                    );
                    return Err(ExceptionCode::IllegalDataValue);
                }

                handler.handle(pdu, ctx)
            }
            None => {
                warn!(function_code, "Unknown function code");
                Err(ExceptionCode::IllegalFunction)
            }
        }
    }

    /// Get the number of registered handlers.
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl std::fmt::Debug for HandlerRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandlerRegistry")
            .field("handlers", &self.handler_info())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::register::RegisterStore;

    fn create_context() -> HandlerContext {
        let registers = Arc::new(RegisterStore::with_defaults());
        HandlerContext::new(1, registers, 1)
    }

    #[test]
    fn test_registry_with_defaults() {
        let registry = HandlerRegistry::with_defaults();

        // Should have all default handlers
        assert!(registry.has_handler(0x01)); // Read Coils
        assert!(registry.has_handler(0x02)); // Read Discrete Inputs
        assert!(registry.has_handler(0x03)); // Read Holding Registers
        assert!(registry.has_handler(0x04)); // Read Input Registers
        assert!(registry.has_handler(0x05)); // Write Single Coil
        assert!(registry.has_handler(0x06)); // Write Single Register
        assert!(registry.has_handler(0x0F)); // Write Multiple Coils
        assert!(registry.has_handler(0x10)); // Write Multiple Registers
        assert!(registry.has_handler(0x16)); // Mask Write Register
        assert!(registry.has_handler(0x17)); // Read/Write Multiple Registers

        assert_eq!(registry.len(), 10);
    }

    #[test]
    fn test_registry_dispatch() {
        let registry = HandlerRegistry::with_defaults();
        let ctx = create_context();

        // Set up test data
        ctx.registers.write_holding_registers(0, &[100, 200, 300]).unwrap();

        // Read holding registers
        let pdu = [0x03, 0x00, 0x00, 0x00, 0x03];
        let response = registry.dispatch(&pdu, &ctx).unwrap();

        assert_eq!(response[0], 0x03);
        assert_eq!(response[1], 6);
    }

    #[test]
    fn test_registry_dispatch_unknown_function() {
        let registry = HandlerRegistry::with_defaults();
        let ctx = create_context();

        // Unknown function code
        let pdu = [0xFF, 0x00, 0x00, 0x00, 0x01];
        let result = registry.dispatch(&pdu, &ctx);

        assert_eq!(result, Err(ExceptionCode::IllegalFunction));
    }

    #[test]
    fn test_registry_dispatch_empty_pdu() {
        let registry = HandlerRegistry::with_defaults();
        let ctx = create_context();

        let result = registry.dispatch(&[], &ctx);
        assert_eq!(result, Err(ExceptionCode::IllegalFunction));
    }

    #[test]
    fn test_registry_unregister() {
        let mut registry = HandlerRegistry::with_defaults();

        // Unregister a handler
        let removed = registry.unregister(0x01);
        assert!(removed.is_some());
        assert!(!registry.has_handler(0x01));

        // Try to unregister again
        let removed = registry.unregister(0x01);
        assert!(removed.is_none());
    }

    #[test]
    fn test_registry_function_codes() {
        let registry = HandlerRegistry::with_defaults();
        let codes = registry.function_codes();

        assert!(codes.contains(&0x01));
        assert!(codes.contains(&0x03));
        assert!(codes.contains(&0x10));
        assert!(codes.contains(&0x16));
        assert_eq!(codes.len(), 10);
    }

    #[test]
    fn test_registry_handler_info() {
        let registry = HandlerRegistry::with_defaults();
        let info = registry.handler_info();

        // Find the Read Coils handler
        let read_coils = info.iter().find(|(fc, _)| *fc == 0x01);
        assert!(read_coils.is_some());
        assert_eq!(read_coils.unwrap().1, "Read Coils");
    }

    // Custom handler for testing registration
    struct CustomHandler;

    impl FunctionHandler for CustomHandler {
        fn function_code(&self) -> u8 {
            0x42
        }

        fn handle(&self, _pdu: &[u8], _ctx: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
            Ok(vec![0x42, 0x00])
        }

        fn name(&self) -> &'static str {
            "Custom Handler"
        }
    }

    #[test]
    fn test_registry_custom_handler() {
        let mut registry = HandlerRegistry::new();
        registry.register(Box::new(CustomHandler));

        let ctx = create_context();
        let pdu = [0x42];
        let response = registry.dispatch(&pdu, &ctx).unwrap();

        assert_eq!(response, vec![0x42, 0x00]);
    }

    #[test]
    fn test_registry_replace_handler() {
        let mut registry = HandlerRegistry::with_defaults();

        // Get the original handler name
        let original_name = registry.get(0x03).map(|h| h.name());
        assert_eq!(original_name, Some("Read Holding Registers"));

        // Replace with custom handler that uses same FC
        struct ReplacementHandler;
        impl FunctionHandler for ReplacementHandler {
            fn function_code(&self) -> u8 {
                0x03
            }
            fn handle(&self, _: &[u8], _: &HandlerContext) -> Result<Vec<u8>, ExceptionCode> {
                Ok(vec![0x03, 0xFF])
            }
            fn name(&self) -> &'static str {
                "Replacement"
            }
        }

        let old = registry.register(Box::new(ReplacementHandler));
        assert!(old.is_some());
        assert_eq!(old.unwrap().name(), "Read Holding Registers");

        // New handler should be active
        let new_name = registry.get(0x03).map(|h| h.name());
        assert_eq!(new_name, Some("Replacement"));
    }
}
