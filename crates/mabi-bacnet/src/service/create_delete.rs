//! BACnet CreateObject and DeleteObject service handlers.
//!
//! Per ASHRAE 135, Clause 15.1 (CreateObject) and 15.8 (DeleteObject):
//!
//! - **CreateObject** (Confirmed, service 10): Dynamically create a new BACnet object.
//! - **DeleteObject** (Confirmed, service 11): Remove a dynamically-created BACnet object.
//!
//! ## Architecture
//!
//! The `ObjectFactory` maps `ObjectType` values to constructor functions, enabling
//! type-safe dynamic object creation without match-arms scattered across handler code.
//! Only types registered with the factory can be created via CreateObject.
//!
//! Device objects and other "permanent" types are excluded from deletion by policy.

use std::collections::HashMap;
use std::sync::Arc;

use crate::apdu::encoding::{ApduDecoder, ApduEncoder};
use crate::apdu::types::{ConfirmedService, ErrorClass, ErrorCode};
use crate::object::property::{BACnetValue, PropertyId};
use crate::object::registry::ObjectRegistry;
use crate::object::traits::ArcObject;
use crate::object::types::{ObjectId, ObjectType};

use super::handler::{ConfirmedServiceHandler, ServiceContext, ServiceResult};

// ── Object Factory ──────────────────────────────────────────────────────────

/// Factory function signature: `(instance: u32, name: String) -> ArcObject`.
type ConstructorFn = fn(u32, String) -> ArcObject;

/// Object factory for dynamically creating BACnet objects.
///
/// Maintains a mapping from `ObjectType` to constructor functions. This is the
/// single source of truth for which types can be dynamically created.
pub struct ObjectFactory {
    constructors: HashMap<ObjectType, ConstructorFn>,
}

impl ObjectFactory {
    /// Create a factory with no registered types.
    pub fn new() -> Self {
        Self {
            constructors: HashMap::new(),
        }
    }

    /// Register a constructor for the given object type.
    pub fn register(&mut self, object_type: ObjectType, constructor: ConstructorFn) {
        self.constructors.insert(object_type, constructor);
    }

    /// Check if a type is supported for dynamic creation.
    pub fn supports(&self, object_type: ObjectType) -> bool {
        self.constructors.contains_key(&object_type)
    }

    /// Create an object of the given type.
    pub fn create(&self, object_type: ObjectType, instance: u32, name: String) -> Option<ArcObject> {
        self.constructors.get(&object_type).map(|ctor| ctor(instance, name))
    }

    /// List all supported object types.
    pub fn supported_types(&self) -> Vec<ObjectType> {
        self.constructors.keys().copied().collect()
    }
}

impl Default for ObjectFactory {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a factory pre-loaded with all standard dynamically-creatable types.
///
/// Excludes Device (type 8) — devices are not dynamically created.
pub fn default_object_factory() -> ObjectFactory {
    use crate::object::event_enrollment::{EventEnrollment, NotificationClass};
    use crate::object::file::FileObject;
    use crate::object::standard::{
        AnalogInput, AnalogOutput, AnalogValue, BinaryInput, BinaryOutput, BinaryValue,
        MultiStateInput, MultiStateOutput, MultiStateValue,
    };
    use crate::object::schedule::{Calendar, Schedule};
    use crate::object::trend_log::TrendLog;

    let mut factory = ObjectFactory::new();

    // Standard I/O types
    factory.register(ObjectType::AnalogInput, |inst, name| {
        Arc::new(AnalogInput::new(inst, name))
    });
    factory.register(ObjectType::AnalogOutput, |inst, name| {
        Arc::new(AnalogOutput::new(inst, name))
    });
    factory.register(ObjectType::AnalogValue, |inst, name| {
        Arc::new(AnalogValue::new(inst, name))
    });
    factory.register(ObjectType::BinaryInput, |inst, name| {
        Arc::new(BinaryInput::new(inst, name))
    });
    factory.register(ObjectType::BinaryOutput, |inst, name| {
        Arc::new(BinaryOutput::new(inst, name))
    });
    factory.register(ObjectType::BinaryValue, |inst, name| {
        Arc::new(BinaryValue::new(inst, name))
    });
    factory.register(ObjectType::MultiStateInput, |inst, name| {
        Arc::new(MultiStateInput::new(inst, name, 4))
    });
    factory.register(ObjectType::MultiStateOutput, |inst, name| {
        Arc::new(MultiStateOutput::new(inst, name, 4))
    });
    factory.register(ObjectType::MultiStateValue, |inst, name| {
        Arc::new(MultiStateValue::new(inst, name, 4))
    });

    // Specialized types
    factory.register(ObjectType::TrendLog, |inst, name| {
        Arc::new(TrendLog::new(inst, &name, 1000))
    });
    factory.register(ObjectType::File, |inst, name| {
        Arc::new(FileObject::new(inst, name))
    });
    factory.register(ObjectType::EventEnrollment, |inst, name| {
        use crate::object::event_enrollment::EventType;
        Arc::new(EventEnrollment::new(inst, name, EventType::ChangeOfValue))
    });
    factory.register(ObjectType::NotificationClass, |inst, name| {
        Arc::new(NotificationClass::new(inst, name))
    });
    factory.register(ObjectType::Schedule, |inst, name| {
        Arc::new(Schedule::new(inst, name))
    });
    factory.register(ObjectType::Calendar, |inst, name| {
        Arc::new(Calendar::new(inst, name))
    });

    factory
}

// ── CreateObject Request ────────────────────────────────────────────────────

/// Decoded CreateObject request.
#[derive(Debug, Clone)]
pub struct CreateObjectRequest {
    /// The type of object to create.
    pub object_type: ObjectType,
    /// Optional specific instance number. If None, auto-assign.
    pub object_instance: Option<u32>,
    /// Optional object name. If None, auto-generate.
    pub object_name: Option<String>,
    /// Initial property values to apply after creation.
    pub initial_values: Vec<(PropertyId, BACnetValue)>,
}

/// Decode a CreateObject request from APDU data.
///
/// BACnet CreateObject-Request encoding (ASHRAE 135, 15.1.1.1):
/// ```text
/// [0] ObjectSpecifier ::= CHOICE {
///     objectType [0] BACnetObjectType,          -- auto-assign instance
///     objectIdentifier [1] BACnetObjectIdentifier -- specific instance
/// }
/// [1] ListOfInitialValues (OPTIONAL) ::= SEQUENCE OF {
///     propertyIdentifier [0] BACnetPropertyIdentifier,
///     propertyArrayIndex [1] Unsigned OPTIONAL,
///     propertyValue      [2] ABSTRACT-SYNTAX.&Type
/// }
/// ```
pub fn decode_create_object_request(data: &[u8]) -> Result<CreateObjectRequest, &'static str> {
    let mut decoder = ApduDecoder::new(data);

    // [0] Object specifier
    if !decoder.is_opening_tag(0) {
        return Err("Expected opening tag [0] for object specifier");
    }
    decoder.read_u8().map_err(|_| "Failed to skip opening tag")?;

    // Peek to determine which choice: context tag [0] = objectType, context tag [1] = objectIdentifier
    let peek = decoder.peek().ok_or("Unexpected end of data")?;
    let context_tag = (peek >> 4) & 0x0F;

    let (object_type, object_instance) = if context_tag == 0 {
        // [0] objectType — just the type enumerated value
        let (_, _, len) = decoder.decode_tag_info().map_err(|_| "Failed to decode type tag")?;
        let type_val = decoder.decode_unsigned(len).map_err(|_| "Failed to decode type value")?;
        let obj_type = ObjectType::from_u16(type_val as u16)
            .ok_or("Unsupported object type")?;
        (obj_type, None)
    } else if context_tag == 1 {
        // [1] objectIdentifier — full ObjectId
        let (_, _, _len) = decoder.decode_tag_info().map_err(|_| "Failed to decode id tag")?;
        let obj_id = decoder.decode_object_identifier().map_err(|_| "Failed to decode object identifier")?;
        (obj_id.object_type, Some(obj_id.instance))
    } else {
        return Err("Invalid object specifier tag");
    };

    // Closing tag [0]
    if !decoder.is_closing_tag(0) {
        return Err("Expected closing tag [0]");
    }
    decoder.read_u8().map_err(|_| "Failed to skip closing tag")?;

    // [1] ListOfInitialValues (optional)
    let mut object_name = None;
    let mut initial_values = Vec::new();

    if !decoder.is_empty() && decoder.is_opening_tag(1) {
        decoder.read_u8().map_err(|_| "Failed to skip opening tag [1]")?;

        while !decoder.is_empty() && !decoder.is_closing_tag(1) {
            // propertyIdentifier [0]
            let (_, _, len) = decoder.decode_tag_info().map_err(|_| "Failed to decode property tag")?;
            let prop_val = decoder.decode_unsigned(len).map_err(|_| "Failed to decode property id")?;
            let property_id = match PropertyId::from_u32(prop_val) {
                Some(id) => id,
                None => {
                    // Skip unknown/vendor properties — advance past value
                    if !decoder.is_empty() {
                        let peek = decoder.peek().unwrap_or(0);
                        let next_tag = (peek >> 4) & 0x0F;
                        if next_tag == 1 && (peek & 0x08) != 0 {
                            let (_, _, len) = decoder.decode_tag_info().map_err(|_| "skip")?;
                            let _ = decoder.decode_unsigned(len);
                        }
                    }
                    // Skip value opening/closing
                    if decoder.is_opening_tag(2) {
                        decoder.read_u8().map_err(|_| "skip")?;
                        skip_to_closing_tag(&mut decoder, 2)?;
                        decoder.read_u8().map_err(|_| "skip")?;
                    }
                    continue;
                }
            };

            // propertyArrayIndex [1] (optional) — skip if present
            if !decoder.is_empty() {
                let peek = decoder.peek().unwrap_or(0);
                let next_tag = (peek >> 4) & 0x0F;
                if next_tag == 1 && (peek & 0x08) != 0 {
                    // Context tag [1] — array index, skip
                    let (_, _, len) = decoder.decode_tag_info().map_err(|_| "Failed to decode array index tag")?;
                    let _ = decoder.decode_unsigned(len);
                }
            }

            // propertyValue [2] — opening/closing tag wrapping the value
            if !decoder.is_opening_tag(2) {
                return Err("Expected opening tag [2] for property value");
            }
            decoder.read_u8().map_err(|_| "Failed to skip opening tag [2]")?;

            // Decode the value based on application tag
            let value = decode_application_value(&mut decoder)?;

            if !decoder.is_closing_tag(2) {
                return Err("Expected closing tag [2] for property value");
            }
            decoder.read_u8().map_err(|_| "Failed to skip closing tag [2]")?;

            // Extract object name if provided
            if property_id == PropertyId::ObjectName {
                if let BACnetValue::CharacterString(ref s) = value {
                    object_name = Some(s.clone());
                }
            }

            initial_values.push((property_id, value));
        }

        if !decoder.is_closing_tag(1) {
            return Err("Expected closing tag [1]");
        }
        decoder.read_u8().map_err(|_| "Failed to skip closing tag [1]")?;
    }

    Ok(CreateObjectRequest {
        object_type,
        object_instance,
        object_name,
        initial_values,
    })
}

/// Skip data until the specified closing tag is found.
fn skip_to_closing_tag(decoder: &mut ApduDecoder<'_>, tag: u8) -> Result<(), &'static str> {
    let mut depth = 0u32;
    while !decoder.is_empty() {
        if depth == 0 && decoder.is_closing_tag(tag) {
            return Ok(());
        }
        let byte = decoder.peek().ok_or("Unexpected end while skipping")?;
        // Opening tag
        if (byte & 0x0F) == 0x0E {
            depth += 1;
            decoder.read_u8().map_err(|_| "skip")?;
            continue;
        }
        // Closing tag
        if (byte & 0x0F) == 0x0F {
            depth = depth.saturating_sub(1);
            decoder.read_u8().map_err(|_| "skip")?;
            continue;
        }
        // Regular tag — decode and skip
        let (_, _, len) = decoder.decode_tag_info().map_err(|_| "skip")?;
        if len > 0 {
            let _ = decoder.read_bytes(len);
        }
    }
    Err("Closing tag not found")
}

/// Decode a single application-tagged value.
fn decode_application_value(decoder: &mut ApduDecoder<'_>) -> Result<BACnetValue, &'static str> {
    let (tag_number, is_context, len) = decoder.decode_tag_info().map_err(|_| "Failed to decode value tag")?;

    if is_context {
        // For context-tagged values in initial value lists, treat as raw unsigned
        let val = decoder.decode_unsigned(len).map_err(|_| "Failed to decode context value")?;
        return Ok(BACnetValue::Unsigned(val));
    }

    match tag_number {
        0 => {
            // Null
            Ok(BACnetValue::Null)
        }
        1 => {
            // Boolean
            Ok(BACnetValue::Boolean(len != 0))
        }
        2 => {
            // Unsigned integer
            let val = decoder.decode_unsigned(len).map_err(|_| "Failed to decode unsigned")?;
            Ok(BACnetValue::Unsigned(val))
        }
        3 => {
            // Signed integer
            let val = decoder.decode_unsigned(len).map_err(|_| "Failed to decode signed")?;
            Ok(BACnetValue::Signed(val as i32))
        }
        4 => {
            // Real (float)
            if len != 4 {
                return Err("Invalid real length");
            }
            let bytes = decoder.read_bytes(4).map_err(|_| "Failed to read float bytes")?;
            let val = f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            Ok(BACnetValue::Real(val))
        }
        5 => {
            // Double
            if len != 8 {
                return Err("Invalid double length");
            }
            let bytes = decoder.read_bytes(8).map_err(|_| "Failed to read double bytes")?;
            let val = f64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3],
                bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            Ok(BACnetValue::Double(val))
        }
        7 => {
            // Character string
            let s = decoder.decode_character_string(len).map_err(|_| "Failed to decode string")?;
            Ok(BACnetValue::CharacterString(s))
        }
        8 => {
            // Bit string — first byte is unused-bits count, rest are data bytes
            let bytes = decoder.read_bytes(len).map_err(|_| "Failed to read bitstring")?;
            if bytes.is_empty() {
                return Ok(BACnetValue::BitString(vec![]));
            }
            let unused_bits = bytes[0] as usize;
            let data = &bytes[1..];
            let total_bits = data.len() * 8 - unused_bits;
            let mut bits = Vec::with_capacity(total_bits);
            for i in 0..total_bits {
                let byte_idx = i / 8;
                let bit_idx = 7 - (i % 8);
                bits.push((data[byte_idx] >> bit_idx) & 1 == 1);
            }
            Ok(BACnetValue::BitString(bits))
        }
        9 => {
            // Enumerated
            let val = decoder.decode_unsigned(len).map_err(|_| "Failed to decode enumerated")?;
            Ok(BACnetValue::Enumerated(val))
        }
        12 => {
            // Object identifier
            let obj_id = decoder.decode_object_identifier().map_err(|_| "Failed to decode object id")?;
            Ok(BACnetValue::ObjectIdentifier(obj_id))
        }
        _ => {
            // Skip unknown tags
            let _ = decoder.read_bytes(len);
            Ok(BACnetValue::Null)
        }
    }
}

// ── DeleteObject Request ────────────────────────────────────────────────────

/// Decoded DeleteObject request.
#[derive(Debug, Clone)]
pub struct DeleteObjectRequest {
    /// The object to delete.
    pub object_id: ObjectId,
}

/// Decode a DeleteObject request from APDU data.
///
/// DeleteObject-Request is simply an ObjectIdentifier (application tag 12).
pub fn decode_delete_object_request(data: &[u8]) -> Result<DeleteObjectRequest, &'static str> {
    let mut decoder = ApduDecoder::new(data);
    let (_, _, _len) = decoder.decode_tag_info().map_err(|_| "Failed to decode tag")?;
    let object_id = decoder.decode_object_identifier().map_err(|_| "Failed to decode object identifier")?;
    Ok(DeleteObjectRequest { object_id })
}

// ── CreateObject Response Encoding ──────────────────────────────────────────

/// Encode a CreateObject-ACK response.
///
/// The response is simply the ObjectIdentifier of the newly created object.
pub fn encode_create_object_ack(object_id: ObjectId) -> Vec<u8> {
    let mut encoder = ApduEncoder::new();
    encoder.encode_object_identifier(object_id);
    encoder.into_bytes()
}

// ── CreateObject Handler ────────────────────────────────────────────────────

/// Handler for BACnet CreateObject (Confirmed Service 10).
///
/// Creates a new BACnet object dynamically and registers it in the object registry.
/// Supports all standard dynamically-creatable types via the embedded `ObjectFactory`.
///
/// Per ASHRAE 135, 15.1.1:
/// - If an object specifier provides only a type, the server auto-assigns the instance.
/// - If it provides a full ObjectIdentifier, that specific instance is used.
/// - If a name is provided in initial values, it must be unique.
/// - The response is the ObjectIdentifier of the created object.
pub struct CreateObjectHandler {
    factory: ObjectFactory,
}

impl CreateObjectHandler {
    /// Create a new handler with the default object factory.
    pub fn new() -> Self {
        Self {
            factory: default_object_factory(),
        }
    }

    /// Create a handler with a custom factory.
    pub fn with_factory(factory: ObjectFactory) -> Self {
        Self { factory }
    }

    /// Execute the creation logic.
    fn create_object(
        &self,
        request: &CreateObjectRequest,
        registry: &Arc<ObjectRegistry>,
    ) -> Result<ObjectId, ServiceResult> {
        // 1. Check if the type is supported
        if !self.factory.supports(request.object_type) {
            return Err(ServiceResult::Error {
                error_class: ErrorClass::Object,
                error_code: ErrorCode::UnsupportedObjectType,
            });
        }

        // 2. Determine instance number
        let instance = if let Some(inst) = request.object_instance {
            // Specific instance requested — check for conflict
            let id = ObjectId::new(request.object_type, inst);
            if registry.contains(&id) {
                return Err(ServiceResult::Error {
                    error_class: ErrorClass::Object,
                    error_code: ErrorCode::ObjectIdentifierAlreadyExists,
                });
            }
            inst
        } else {
            // Auto-assign
            registry.next_available_instance(request.object_type).ok_or_else(|| {
                ServiceResult::Error {
                    error_class: ErrorClass::Resources,
                    error_code: ErrorCode::NoSpaceForObject,
                }
            })?
        };

        // 3. Determine name
        let name = if let Some(ref n) = request.object_name {
            // Check uniqueness
            if registry.contains_name(n) {
                return Err(ServiceResult::Error {
                    error_class: ErrorClass::Object,
                    error_code: ErrorCode::DuplicateName,
                });
            }
            n.clone()
        } else {
            // Auto-generate name
            let type_prefix = object_type_prefix(request.object_type);
            format!("{}_{}", type_prefix, instance)
        };

        // 4. Create the object
        let object = self.factory.create(request.object_type, instance, name).ok_or_else(|| {
            ServiceResult::Error {
                error_class: ErrorClass::Object,
                error_code: ErrorCode::UnsupportedObjectType,
            }
        })?;

        let object_id = object.object_identifier();

        // 5. Apply initial property values (skip ObjectName — already set)
        for (prop_id, value) in &request.initial_values {
            if *prop_id == PropertyId::ObjectName || *prop_id == PropertyId::ObjectIdentifier {
                continue; // Already handled
            }
            // Best-effort: ignore errors on initial values
            let _ = object.write_property(*prop_id, value.clone());
        }

        // 6. Register
        registry.register(object);

        Ok(object_id)
    }
}

impl ConfirmedServiceHandler for CreateObjectHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::CreateObject
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        let request = match decode_create_object_request(data) {
            Ok(r) => r,
            Err(_) => return ServiceResult::missing_required_parameter(),
        };

        match self.create_object(&request, &ctx.objects) {
            Ok(object_id) => {
                ServiceResult::ComplexAck(encode_create_object_ack(object_id))
            }
            Err(result) => result,
        }
    }

    fn name(&self) -> &'static str {
        "CreateObject"
    }

    fn min_data_length(&self) -> usize {
        4 // Minimum: opening tag + type tag + value + closing tag
    }
}

// ── DeleteObject Handler ────────────────────────────────────────────────────

/// Set of object types that cannot be dynamically deleted.
const NON_DELETABLE_TYPES: &[ObjectType] = &[
    ObjectType::Device,
];

/// Handler for BACnet DeleteObject (Confirmed Service 11).
///
/// Removes a dynamically-created object from the registry.
///
/// Per ASHRAE 135, 15.8.1:
/// - Device objects cannot be deleted.
/// - Returns SimpleAck on success.
/// - Returns ERROR(object, objectDeletionNotPermitted) for protected types.
pub struct DeleteObjectHandler;

impl DeleteObjectHandler {
    pub fn new() -> Self {
        Self
    }
}

impl ConfirmedServiceHandler for DeleteObjectHandler {
    fn service_choice(&self) -> ConfirmedService {
        ConfirmedService::DeleteObject
    }

    fn handle(&self, data: &[u8], ctx: &ServiceContext) -> ServiceResult {
        let request = match decode_delete_object_request(data) {
            Ok(r) => r,
            Err(_) => return ServiceResult::missing_required_parameter(),
        };

        // Check if the type is deletable
        if NON_DELETABLE_TYPES.contains(&request.object_id.object_type) {
            return ServiceResult::Error {
                error_class: ErrorClass::Object,
                error_code: ErrorCode::ObjectDeletionNotPermitted,
            };
        }

        // Check if object exists
        if !ctx.objects.contains(&request.object_id) {
            return ServiceResult::unknown_object();
        }

        // Remove it
        ctx.objects.unregister(&request.object_id);

        ServiceResult::SimpleAck
    }

    fn name(&self) -> &'static str {
        "DeleteObject"
    }

    fn min_data_length(&self) -> usize {
        4 // ObjectIdentifier = 4 bytes
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Returns a short prefix for auto-generated object names.
fn object_type_prefix(object_type: ObjectType) -> &'static str {
    match object_type {
        ObjectType::AnalogInput => "AI",
        ObjectType::AnalogOutput => "AO",
        ObjectType::AnalogValue => "AV",
        ObjectType::BinaryInput => "BI",
        ObjectType::BinaryOutput => "BO",
        ObjectType::BinaryValue => "BV",
        ObjectType::MultiStateInput => "MSI",
        ObjectType::MultiStateOutput => "MSO",
        ObjectType::MultiStateValue => "MSV",
        ObjectType::TrendLog => "TL",
        ObjectType::File => "FILE",
        ObjectType::EventEnrollment => "EE",
        ObjectType::NotificationClass => "NC",
        ObjectType::Schedule => "SCHED",
        ObjectType::Calendar => "CAL",
        _ => "OBJ",
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::registry::ObjectRegistry;
    use crate::object::standard::AnalogInput;

    fn make_ctx(registry: &Arc<ObjectRegistry>) -> ServiceContext {
        ServiceContext {
            objects: registry.clone(),
            device_instance: 1234,
            invoke_id: Some(1),
            max_apdu_length: 1476,
            source_address: None,
        }
    }

    #[test]
    fn test_handler_creation() {
        let create = CreateObjectHandler::new();
        assert_eq!(create.service_choice(), ConfirmedService::CreateObject);
        assert_eq!(create.name(), "CreateObject");

        let delete = DeleteObjectHandler::new();
        assert_eq!(delete.service_choice(), ConfirmedService::DeleteObject);
        assert_eq!(delete.name(), "DeleteObject");
    }

    #[test]
    fn test_object_factory_default() {
        let factory = default_object_factory();

        assert!(factory.supports(ObjectType::AnalogInput));
        assert!(factory.supports(ObjectType::AnalogOutput));
        assert!(factory.supports(ObjectType::AnalogValue));
        assert!(factory.supports(ObjectType::BinaryInput));
        assert!(factory.supports(ObjectType::BinaryOutput));
        assert!(factory.supports(ObjectType::BinaryValue));
        assert!(factory.supports(ObjectType::MultiStateInput));
        assert!(factory.supports(ObjectType::MultiStateOutput));
        assert!(factory.supports(ObjectType::MultiStateValue));
        assert!(factory.supports(ObjectType::TrendLog));
        assert!(factory.supports(ObjectType::File));
        assert!(factory.supports(ObjectType::EventEnrollment));
        assert!(factory.supports(ObjectType::NotificationClass));
        assert!(factory.supports(ObjectType::Schedule));
        assert!(factory.supports(ObjectType::Calendar));

        // Device should NOT be creatable
        assert!(!factory.supports(ObjectType::Device));
    }

    #[test]
    fn test_factory_create_object() {
        let factory = default_object_factory();
        let obj = factory.create(ObjectType::AnalogInput, 42, "TestAI".to_string()).unwrap();
        assert_eq!(obj.object_identifier(), ObjectId::new(ObjectType::AnalogInput, 42));
        assert_eq!(obj.object_name(), "TestAI");
    }

    #[test]
    fn test_factory_unsupported_type() {
        let factory = default_object_factory();
        assert!(factory.create(ObjectType::Device, 0, "Dev".to_string()).is_none());
    }

    #[test]
    fn test_create_object_auto_instance() {
        let handler = CreateObjectHandler::new();
        let registry = Arc::new(ObjectRegistry::new());

        // Register an existing AI at instance 0
        registry.register(Arc::new(AnalogInput::new(0, "AI_0")));

        let request = CreateObjectRequest {
            object_type: ObjectType::AnalogInput,
            object_instance: None,
            object_name: Some("NewAI".to_string()),
            initial_values: vec![],
        };

        let result = handler.create_object(&request, &registry);
        let obj_id = result.unwrap();
        assert_eq!(obj_id.object_type, ObjectType::AnalogInput);
        assert_eq!(obj_id.instance, 1); // Auto-assigned next instance
        assert!(registry.contains(&obj_id));
        assert_eq!(registry.get(&obj_id).unwrap().object_name(), "NewAI");
    }

    #[test]
    fn test_create_object_specific_instance() {
        let handler = CreateObjectHandler::new();
        let registry = Arc::new(ObjectRegistry::new());

        let request = CreateObjectRequest {
            object_type: ObjectType::BinaryInput,
            object_instance: Some(100),
            object_name: Some("BI_100".to_string()),
            initial_values: vec![],
        };

        let result = handler.create_object(&request, &registry);
        let obj_id = result.unwrap();
        assert_eq!(obj_id, ObjectId::new(ObjectType::BinaryInput, 100));
    }

    #[test]
    fn test_create_object_duplicate_instance() {
        let handler = CreateObjectHandler::new();
        let registry = Arc::new(ObjectRegistry::new());
        registry.register(Arc::new(AnalogInput::new(5, "AI_5")));

        let request = CreateObjectRequest {
            object_type: ObjectType::AnalogInput,
            object_instance: Some(5),
            object_name: Some("DuplicateAI".to_string()),
            initial_values: vec![],
        };

        let result = handler.create_object(&request, &registry);
        assert!(result.is_err());
        if let Err(ServiceResult::Error { error_code, .. }) = result {
            assert_eq!(error_code, ErrorCode::ObjectIdentifierAlreadyExists);
        }
    }

    #[test]
    fn test_create_object_duplicate_name() {
        let handler = CreateObjectHandler::new();
        let registry = Arc::new(ObjectRegistry::new());
        registry.register(Arc::new(AnalogInput::new(0, "TakenName")));

        let request = CreateObjectRequest {
            object_type: ObjectType::AnalogInput,
            object_instance: None,
            object_name: Some("TakenName".to_string()),
            initial_values: vec![],
        };

        let result = handler.create_object(&request, &registry);
        assert!(result.is_err());
        if let Err(ServiceResult::Error { error_code, .. }) = result {
            assert_eq!(error_code, ErrorCode::DuplicateName);
        }
    }

    #[test]
    fn test_create_object_unsupported_type() {
        let handler = CreateObjectHandler::new();
        let registry = Arc::new(ObjectRegistry::new());

        let request = CreateObjectRequest {
            object_type: ObjectType::Device,
            object_instance: None,
            object_name: None,
            initial_values: vec![],
        };

        let result = handler.create_object(&request, &registry);
        assert!(result.is_err());
        if let Err(ServiceResult::Error { error_code, .. }) = result {
            assert_eq!(error_code, ErrorCode::UnsupportedObjectType);
        }
    }

    #[test]
    fn test_create_object_auto_name() {
        let handler = CreateObjectHandler::new();
        let registry = Arc::new(ObjectRegistry::new());

        let request = CreateObjectRequest {
            object_type: ObjectType::BinaryValue,
            object_instance: None,
            object_name: None, // No name provided
            initial_values: vec![],
        };

        let result = handler.create_object(&request, &registry).unwrap();
        let obj = registry.get(&result).unwrap();
        assert_eq!(obj.object_name(), "BV_0");
    }

    #[test]
    fn test_create_object_with_initial_values() {
        let handler = CreateObjectHandler::new();
        let registry = Arc::new(ObjectRegistry::new());

        let request = CreateObjectRequest {
            object_type: ObjectType::AnalogInput,
            object_instance: None,
            object_name: Some("TempSensor".to_string()),
            initial_values: vec![
                // Units: 62 = degreesCelsius (stored in PropertyStore, round-trips)
                (PropertyId::Units, BACnetValue::Enumerated(62)),
            ],
        };

        let result = handler.create_object(&request, &registry).unwrap();
        let obj = registry.get(&result).unwrap();
        let units = obj.read_property(PropertyId::Units).unwrap();
        assert_eq!(units, BACnetValue::Enumerated(62));
    }

    #[test]
    fn test_create_object_handler_apdu() {
        let handler = CreateObjectHandler::new();
        let registry = Arc::new(ObjectRegistry::new());
        let ctx = make_ctx(&registry);

        // Encode a CreateObject request for AnalogInput (type 0)
        // [0] opening, context[0] unsigned 0, [0] closing
        let data = [
            0x0E, // opening tag [0]
            0x09, 0x00, // context tag [0] len=1, value=0 (AnalogInput)
            0x0F, // closing tag [0]
        ];

        let result = handler.handle(&data, &ctx);
        match result {
            ServiceResult::ComplexAck(ack_data) => {
                // Should contain ObjectIdentifier for AnalogInput:0
                assert!(!ack_data.is_empty());
            }
            other => panic!("Expected ComplexAck, got {:?}", other),
        }

        // Verify object was created
        let obj_id = ObjectId::new(ObjectType::AnalogInput, 0);
        assert!(registry.contains(&obj_id));
    }

    #[test]
    fn test_delete_object_success() {
        let handler = DeleteObjectHandler::new();
        let registry = Arc::new(ObjectRegistry::new());
        registry.register(Arc::new(AnalogInput::new(5, "AI_5")));
        let ctx = make_ctx(&registry);

        // Encode DeleteObject request: ObjectIdentifier application tag 12
        let obj_id = ObjectId::new(ObjectType::AnalogInput, 5);
        let encoded = obj_id.encode();
        let data = [
            0xC4, // application tag 12, length 4
            (encoded >> 24) as u8,
            (encoded >> 16) as u8,
            (encoded >> 8) as u8,
            encoded as u8,
        ];

        let result = handler.handle(&data, &ctx);
        assert!(matches!(result, ServiceResult::SimpleAck));
        assert!(!registry.contains(&obj_id));
    }

    #[test]
    fn test_delete_device_not_permitted() {
        let handler = DeleteObjectHandler::new();
        let registry = Arc::new(ObjectRegistry::new());
        let ctx = make_ctx(&registry);

        // Try to delete a Device object
        let obj_id = ObjectId::new(ObjectType::Device, 1234);
        let encoded = obj_id.encode();
        let data = [
            0xC4,
            (encoded >> 24) as u8,
            (encoded >> 16) as u8,
            (encoded >> 8) as u8,
            encoded as u8,
        ];

        let result = handler.handle(&data, &ctx);
        match result {
            ServiceResult::Error { error_code, .. } => {
                assert_eq!(error_code, ErrorCode::ObjectDeletionNotPermitted);
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn test_delete_nonexistent_object() {
        let handler = DeleteObjectHandler::new();
        let registry = Arc::new(ObjectRegistry::new());
        let ctx = make_ctx(&registry);

        let obj_id = ObjectId::new(ObjectType::AnalogInput, 999);
        let encoded = obj_id.encode();
        let data = [
            0xC4,
            (encoded >> 24) as u8,
            (encoded >> 16) as u8,
            (encoded >> 8) as u8,
            encoded as u8,
        ];

        let result = handler.handle(&data, &ctx);
        match result {
            ServiceResult::Error { error_code, .. } => {
                assert_eq!(error_code, ErrorCode::UnknownObject);
            }
            other => panic!("Expected Error, got {:?}", other),
        }
    }

    #[test]
    fn test_next_available_instance() {
        let registry = ObjectRegistry::new();
        assert_eq!(registry.next_available_instance(ObjectType::AnalogInput), Some(0));

        registry.register(Arc::new(AnalogInput::new(0, "AI_0")));
        assert_eq!(registry.next_available_instance(ObjectType::AnalogInput), Some(1));

        registry.register(Arc::new(AnalogInput::new(5, "AI_5")));
        assert_eq!(registry.next_available_instance(ObjectType::AnalogInput), Some(6));
    }

    #[test]
    fn test_create_then_delete_roundtrip() {
        let create_handler = CreateObjectHandler::new();
        let delete_handler = DeleteObjectHandler::new();
        let registry = Arc::new(ObjectRegistry::new());

        // Create
        let request = CreateObjectRequest {
            object_type: ObjectType::AnalogValue,
            object_instance: None,
            object_name: Some("TempSetpoint".to_string()),
            initial_values: vec![],
        };
        let obj_id = create_handler.create_object(&request, &registry).unwrap();
        assert!(registry.contains(&obj_id));
        assert_eq!(registry.len(), 1);

        // Delete
        registry.unregister(&obj_id);
        assert!(!registry.contains(&obj_id));
        assert_eq!(registry.len(), 0);

        // Can create again at same type
        let request2 = CreateObjectRequest {
            object_type: ObjectType::AnalogValue,
            object_instance: None,
            object_name: Some("TempSetpoint2".to_string()),
            initial_values: vec![],
        };
        let obj_id2 = create_handler.create_object(&request2, &registry).unwrap();
        assert_eq!(obj_id2.instance, 0); // Should start fresh
    }

    #[test]
    fn test_decode_create_object_type_only() {
        // CreateObject with just object type (AnalogInput = 0)
        let data = [
            0x0E, // opening tag [0]
            0x09, 0x00, // context tag [0] len=1, value=0
            0x0F, // closing tag [0]
        ];

        let request = decode_create_object_request(&data).unwrap();
        assert_eq!(request.object_type, ObjectType::AnalogInput);
        assert!(request.object_instance.is_none());
        assert!(request.object_name.is_none());
        assert!(request.initial_values.is_empty());
    }

    #[test]
    fn test_decode_create_object_with_identifier() {
        // CreateObject with specific ObjectIdentifier
        // ObjectIdentifier for AnalogInput:42
        let obj_id = ObjectId::new(ObjectType::AnalogInput, 42);
        let encoded = obj_id.encode();

        let data = [
            0x0E, // opening tag [0]
            0x1C, // context tag [1], length 4
            (encoded >> 24) as u8,
            (encoded >> 16) as u8,
            (encoded >> 8) as u8,
            encoded as u8,
            0x0F, // closing tag [0]
        ];

        let request = decode_create_object_request(&data).unwrap();
        assert_eq!(request.object_type, ObjectType::AnalogInput);
        assert_eq!(request.object_instance, Some(42));
    }

    #[test]
    fn test_decode_delete_object() {
        let obj_id = ObjectId::new(ObjectType::BinaryOutput, 10);
        let encoded = obj_id.encode();
        let data = [
            0xC4, // application tag 12, length 4
            (encoded >> 24) as u8,
            (encoded >> 16) as u8,
            (encoded >> 8) as u8,
            encoded as u8,
        ];

        let request = decode_delete_object_request(&data).unwrap();
        assert_eq!(request.object_id, obj_id);
    }

    #[test]
    fn test_encode_create_object_ack() {
        let obj_id = ObjectId::new(ObjectType::AnalogInput, 7);
        let ack = encode_create_object_ack(obj_id);
        assert!(!ack.is_empty());
        // Should be an application-tagged ObjectIdentifier
        assert_eq!(ack[0] & 0xF0, 0xC0); // tag 12
    }

    #[test]
    fn test_create_multiple_objects_sequential() {
        let handler = CreateObjectHandler::new();
        let registry = Arc::new(ObjectRegistry::new());

        for i in 0..5 {
            let request = CreateObjectRequest {
                object_type: ObjectType::AnalogInput,
                object_instance: None,
                object_name: Some(format!("AI_{}", i)),
                initial_values: vec![],
            };
            let obj_id = handler.create_object(&request, &registry).unwrap();
            assert_eq!(obj_id.instance, i);
        }

        assert_eq!(registry.count_by_type(ObjectType::AnalogInput), 5);
    }

    #[test]
    fn test_object_type_prefix() {
        assert_eq!(object_type_prefix(ObjectType::AnalogInput), "AI");
        assert_eq!(object_type_prefix(ObjectType::BinaryOutput), "BO");
        assert_eq!(object_type_prefix(ObjectType::MultiStateValue), "MSV");
        assert_eq!(object_type_prefix(ObjectType::TrendLog), "TL");
        assert_eq!(object_type_prefix(ObjectType::File), "FILE");
        assert_eq!(object_type_prefix(ObjectType::EventEnrollment), "EE");
        assert_eq!(object_type_prefix(ObjectType::NotificationClass), "NC");
    }
}
