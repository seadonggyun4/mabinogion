//! Transport-independent request execution services.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::context::{AddressSpace, RequestTarget, ServerContext, SharedAddressSpace};
use crate::core::{
    build_exception_pdu, parse_semantic_request, ExceptionCode, FunctionCode, RequestPdu,
    ResponsePdu, SemanticRequest, SemanticResponse,
};
use crate::error::ModbusError;
use crate::handler::{HandlerContext, HandlerRegistry};
use crate::handler::FunctionHandler;
use crate::types::WordOrder;

#[derive(Clone)]
enum ServiceTargets {
    Unicast(RequestTarget),
    Broadcast(Vec<RequestTarget>),
}

/// A transport-neutral Modbus service request.
#[derive(Clone)]
pub struct ServiceRequest {
    unit_id: u8,
    transaction_id: u16,
    pdu: RequestPdu,
    targets: ServiceTargets,
}

impl ServiceRequest {
    pub fn new(unit_id: u8, transaction_id: u16, pdu: RequestPdu, target: RequestTarget) -> Self {
        Self {
            unit_id,
            transaction_id,
            pdu,
            targets: ServiceTargets::Unicast(target),
        }
    }

    pub fn broadcast(transaction_id: u16, pdu: RequestPdu, targets: Vec<RequestTarget>) -> Self {
        Self {
            unit_id: 0,
            transaction_id,
            pdu,
            targets: ServiceTargets::Broadcast(targets),
        }
    }

    pub fn unit_id(&self) -> u8 {
        self.unit_id
    }

    pub fn transaction_id(&self) -> u16 {
        self.transaction_id
    }

    pub fn pdu(&self) -> &RequestPdu {
        &self.pdu
    }

    pub fn is_broadcast(&self) -> bool {
        matches!(self.targets, ServiceTargets::Broadcast(_))
    }

    pub fn target(&self) -> Option<&RequestTarget> {
        match &self.targets {
            ServiceTargets::Unicast(target) => Some(target),
            ServiceTargets::Broadcast(_) => None,
        }
    }
}

#[derive(Clone, Copy)]
enum ServiceTargetsRef<'a> {
    Unicast(&'a RequestTarget),
    Broadcast(&'a [RequestTarget]),
}

/// Borrowed transport request used by the shared fast path.
#[derive(Clone, Copy)]
pub struct ServiceRequestView<'a> {
    unit_id: u8,
    transaction_id: u16,
    pdu: &'a [u8],
    targets: ServiceTargetsRef<'a>,
}

impl<'a> ServiceRequestView<'a> {
    pub fn new(unit_id: u8, transaction_id: u16, pdu: &'a [u8], target: &'a RequestTarget) -> Self {
        Self {
            unit_id,
            transaction_id,
            pdu,
            targets: ServiceTargetsRef::Unicast(target),
        }
    }

    pub fn broadcast(transaction_id: u16, pdu: &'a [u8], targets: &'a [RequestTarget]) -> Self {
        Self {
            unit_id: 0,
            transaction_id,
            pdu,
            targets: ServiceTargetsRef::Broadcast(targets),
        }
    }

    pub fn unit_id(&self) -> u8 {
        self.unit_id
    }

    pub fn transaction_id(&self) -> u16 {
        self.transaction_id
    }

    pub fn pdu(&self) -> &'a [u8] {
        self.pdu
    }

    pub fn is_broadcast(&self) -> bool {
        matches!(self.targets, ServiceTargetsRef::Broadcast(_))
    }

    pub fn target(&self) -> Option<&'a RequestTarget> {
        match self.targets {
            ServiceTargetsRef::Unicast(target) => Some(target),
            ServiceTargetsRef::Broadcast(_) => None,
        }
    }

    pub fn targets(&self) -> &'a [RequestTarget] {
        match self.targets {
            ServiceTargetsRef::Unicast(target) => std::slice::from_ref(target),
            ServiceTargetsRef::Broadcast(targets) => targets,
        }
    }

    fn to_owned(self) -> Result<ServiceRequest, ModbusError> {
        let pdu = RequestPdu::new(self.pdu.to_vec())?;
        Ok(match self.targets {
            ServiceTargetsRef::Unicast(target) => {
                ServiceRequest::new(self.unit_id, self.transaction_id, pdu, target.clone())
            }
            ServiceTargetsRef::Broadcast(targets) => {
                ServiceRequest::broadcast(self.transaction_id, pdu, targets.to_vec())
            }
        })
    }
}

/// Canonical transport-neutral service result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceOutcome {
    Reply(ResponsePdu),
    Ignore,
    Exception(ExceptionCode),
}

impl ServiceOutcome {
    fn into_transport_disposition(self, function_code: u8, is_broadcast: bool) -> TransportDisposition {
        let disposition = match self {
            Self::Reply(response) => TransportDisposition::Reply(response),
            Self::Ignore => TransportDisposition::Ignore,
            Self::Exception(code) => {
                let response = ResponsePdu::new(build_exception_pdu(function_code, code))
                    .expect("exception PDUs are always valid");
                TransportDisposition::Reply(response)
            }
        };

        if is_broadcast {
            match disposition {
                TransportDisposition::Reply(response)
                | TransportDisposition::BroadcastSuppressed(response) => {
                    TransportDisposition::BroadcastSuppressed(response)
                }
                TransportDisposition::Ignore => TransportDisposition::Ignore,
            }
        } else {
            disposition
        }
    }
}

/// Metadata describing a registered custom Modbus extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionMetadata {
    pub function_code: u8,
    pub name: String,
    pub supports_broadcast: bool,
}

/// Typed extension request decoded from an unknown function code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionRequest {
    function_code: u8,
    payload: Vec<u8>,
    transaction_id: u16,
    unit_id: u8,
    is_broadcast: bool,
}

impl ExtensionRequest {
    pub fn new(
        function_code: u8,
        payload: impl Into<Vec<u8>>,
        transaction_id: u16,
        unit_id: u8,
        is_broadcast: bool,
    ) -> Self {
        Self {
            function_code,
            payload: payload.into(),
            transaction_id,
            unit_id,
            is_broadcast,
        }
    }

    pub fn function_code(&self) -> u8 {
        self.function_code
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn transaction_id(&self) -> u16 {
        self.transaction_id
    }

    pub fn unit_id(&self) -> u8 {
        self.unit_id
    }

    pub fn is_broadcast(&self) -> bool {
        self.is_broadcast
    }

    pub fn raw_pdu(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.payload.len() + 1);
        bytes.push(self.function_code);
        bytes.extend_from_slice(&self.payload);
        bytes
    }
}

/// Execution context passed to custom extension handlers.
#[derive(Clone)]
pub struct ExtensionContext {
    unit_id: u8,
    transaction_id: u16,
    address_space: SharedAddressSpace,
    word_order: WordOrder,
    is_broadcast: bool,
}

impl ExtensionContext {
    fn from_target(
        unit_id: u8,
        transaction_id: u16,
        target: &RequestTarget,
        is_broadcast: bool,
    ) -> Self {
        Self {
            unit_id,
            transaction_id,
            address_space: target.address_space(),
            word_order: target.word_order(),
            is_broadcast,
        }
    }

    pub fn unit_id(&self) -> u8 {
        self.unit_id
    }

    pub fn transaction_id(&self) -> u16 {
        self.transaction_id
    }

    pub fn address_space(&self) -> SharedAddressSpace {
        self.address_space.clone()
    }

    pub fn word_order(&self) -> WordOrder {
        self.word_order
    }

    pub fn is_broadcast(&self) -> bool {
        self.is_broadcast
    }

    pub fn handler_context(&self) -> HandlerContext {
        HandlerContext::with_word_order(
            self.unit_id,
            self.address_space(),
            self.transaction_id,
            self.word_order,
        )
    }
}

/// Object-safe registry surface for custom function extensions.
pub trait ExtensionHandler: Send + Sync {
    fn function_code(&self) -> u8;

    fn metadata(&self) -> ExtensionMetadata;

    fn decode(
        &self,
        raw_pdu: &[u8],
        unit_id: u8,
        transaction_id: u16,
        is_broadcast: bool,
    ) -> Result<ExtensionRequest, ExceptionCode>;

    fn execute(&self, request: &ExtensionRequest, context: &ExtensionContext) -> ServiceOutcome;
}

/// Typed registry for custom function extensions.
#[derive(Default, Clone)]
pub struct ExtensionRegistry {
    handlers: HashMap<u8, Arc<dyn ExtensionHandler>>,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn register(
        &mut self,
        handler: Arc<dyn ExtensionHandler>,
    ) -> Option<Arc<dyn ExtensionHandler>> {
        self.handlers.insert(handler.function_code(), handler)
    }

    pub fn unregister(&mut self, function_code: u8) -> Option<Arc<dyn ExtensionHandler>> {
        self.handlers.remove(&function_code)
    }

    pub fn get(&self, function_code: u8) -> Option<&Arc<dyn ExtensionHandler>> {
        self.handlers.get(&function_code)
    }

    pub fn has_handler(&self, function_code: u8) -> bool {
        self.handlers.contains_key(&function_code)
    }

    pub fn metadata(&self) -> Vec<ExtensionMetadata> {
        self.handlers.values().map(|handler| handler.metadata()).collect()
    }

    fn dispatch(
        &self,
        unit_id: u8,
        transaction_id: u16,
        raw_pdu: &[u8],
        targets: ServiceTargetsRef<'_>,
    ) -> ServiceOutcome {
        let function_code = match raw_pdu.first().copied() {
            Some(code) => code,
            None => return ServiceOutcome::Exception(ExceptionCode::IllegalDataValue),
        };
        let handler = match self.get(function_code) {
            Some(handler) => handler,
            None => return ServiceOutcome::Exception(ExceptionCode::IllegalFunction),
        };
        let is_broadcast = matches!(targets, ServiceTargetsRef::Broadcast(_));
        let request = match handler.decode(raw_pdu, unit_id, transaction_id, is_broadcast) {
            Ok(request) => request,
            Err(error) => return ServiceOutcome::Exception(error),
        };

        match targets {
            ServiceTargetsRef::Broadcast(targets) => {
                let metadata = handler.metadata();
                if !metadata.supports_broadcast {
                    return ServiceOutcome::Exception(ExceptionCode::IllegalFunction);
                }

                let mut outcome = ServiceOutcome::Ignore;
                for target in targets {
                    let context =
                        ExtensionContext::from_target(unit_id, transaction_id, target, true);
                    outcome = handler.execute(&request, &context);
                    if matches!(outcome, ServiceOutcome::Exception(_)) {
                        return outcome;
                    }
                }
                outcome
            }
            ServiceTargetsRef::Unicast(target) => {
                let context = ExtensionContext::from_target(unit_id, transaction_id, target, false);
                handler.execute(&request, &context)
            }
        }
    }
}

impl std::fmt::Debug for ExtensionRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionRegistry")
            .field("metadata", &self.metadata())
            .finish()
    }
}

struct LegacyExtensionHandler {
    handler: Arc<dyn FunctionHandler>,
}

impl LegacyExtensionHandler {
    fn new(handler: Arc<dyn FunctionHandler>) -> Self {
        Self { handler }
    }
}

impl ExtensionHandler for LegacyExtensionHandler {
    fn function_code(&self) -> u8 {
        self.handler.function_code()
    }

    fn metadata(&self) -> ExtensionMetadata {
        ExtensionMetadata {
            function_code: self.handler.function_code(),
            name: self.handler.name().to_string(),
            supports_broadcast: self.handler.supports_broadcast(),
        }
    }

    fn decode(
        &self,
        raw_pdu: &[u8],
        unit_id: u8,
        transaction_id: u16,
        is_broadcast: bool,
    ) -> Result<ExtensionRequest, ExceptionCode> {
        if raw_pdu.len() < self.handler.min_pdu_length() {
            return Err(ExceptionCode::IllegalDataValue);
        }
        if is_broadcast && !self.handler.supports_broadcast() {
            return Err(ExceptionCode::IllegalFunction);
        }
        let function_code = *raw_pdu.first().ok_or(ExceptionCode::IllegalDataValue)?;
        Ok(ExtensionRequest::new(
            function_code,
            raw_pdu[1..].to_vec(),
            transaction_id,
            unit_id,
            is_broadcast,
        ))
    }

    fn execute(&self, request: &ExtensionRequest, context: &ExtensionContext) -> ServiceOutcome {
        match self
            .handler
            .handle(&request.raw_pdu(), &context.handler_context())
        {
            Ok(response) => match ResponsePdu::new(response) {
                Ok(response) => ServiceOutcome::Reply(response),
                Err(error) => ServiceOutcome::Exception(map_modbus_error(error)),
            },
            Err(error) => ServiceOutcome::Exception(error),
        }
    }
}

impl From<HandlerRegistry> for ExtensionRegistry {
    fn from(value: HandlerRegistry) -> Self {
        let mut registry = Self::new();
        for function_code in value.function_codes() {
            if let Some(handler) = value.get(function_code) {
                registry.register(Arc::new(LegacyExtensionHandler::new(Arc::clone(handler))));
            }
        }
        registry
    }
}

/// Transport-neutral Modbus request executor.
pub trait ModbusService: Send + Sync {
    fn call(&self, request: &ServiceRequest) -> ServiceOutcome;

    fn call_view(&self, request: ServiceRequestView<'_>) -> ServiceOutcome {
        match request.to_owned() {
            Ok(request) => self.call(&request),
            Err(error) => ServiceOutcome::Exception(map_modbus_error(error)),
        }
    }
}

/// Behavior for requests that target an unknown unit.
#[derive(Debug, Clone, Copy)]
pub enum UnknownUnitBehavior {
    Ignore,
    Exception(ExceptionCode),
}

/// Shared transport policy used by both TCP and RTU request loops.
#[derive(Debug, Clone, Copy)]
pub struct TransportServicePolicy {
    pub request_timeout: Option<Duration>,
    pub unknown_unit_behavior: UnknownUnitBehavior,
}

impl TransportServicePolicy {
    pub fn new(unknown_unit_behavior: UnknownUnitBehavior) -> Self {
        Self {
            request_timeout: None,
            unknown_unit_behavior,
        }
    }

    pub fn with_request_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.request_timeout = timeout;
        self
    }
}

/// Shared result returned by the transport-neutral request skeleton.
#[derive(Debug, Clone)]
pub struct ExecutedTransportRequest {
    pub function_code: u8,
    pub unit_id: u8,
    pub timed_out: bool,
    pub disposition: TransportDisposition,
}

impl ExecutedTransportRequest {
    pub fn is_broadcast(&self) -> bool {
        matches!(
            self.disposition,
            TransportDisposition::BroadcastSuppressed(_)
        )
    }

    pub fn response_pdu(&self) -> Option<&ResponsePdu> {
        match &self.disposition {
            TransportDisposition::Reply(response)
            | TransportDisposition::BroadcastSuppressed(response) => Some(response),
            TransportDisposition::Ignore => None,
        }
    }
}

/// Transport disposition after semantic execution.
#[derive(Debug, Clone)]
pub enum TransportDisposition {
    Reply(ResponsePdu),
    BroadcastSuppressed(ResponsePdu),
    Ignore,
}

/// Execute a raw transport request against the shared semantic service core.
pub async fn execute_transport_request(
    service: &dyn ModbusService,
    server_context: &ServerContext,
    unit_id: u8,
    transaction_id: u16,
    raw_pdu: &[u8],
    policy: TransportServicePolicy,
) -> ExecutedTransportRequest {
    let function_code = raw_pdu.first().copied().unwrap_or(0);
    let is_broadcast = unit_id == 0;

    if raw_pdu.is_empty() {
        return ExecutedTransportRequest {
            function_code,
            unit_id,
            timed_out: false,
            disposition: ServiceOutcome::Exception(ExceptionCode::IllegalDataValue)
                .into_transport_disposition(function_code, is_broadcast),
        };
    }

    let broadcast_targets;
    let unicast_target;
    let service_request = if is_broadcast {
        broadcast_targets = server_context.broadcast_targets();
        ServiceRequestView::broadcast(transaction_id, raw_pdu, &broadcast_targets)
    } else if let Some(target) = server_context.target_for_unit(unit_id) {
        unicast_target = target;
        ServiceRequestView::new(unit_id, transaction_id, raw_pdu, &unicast_target)
    } else {
        return ExecutedTransportRequest {
            function_code,
            unit_id,
            timed_out: false,
            disposition: match policy.unknown_unit_behavior {
                UnknownUnitBehavior::Ignore => ServiceOutcome::Ignore,
                UnknownUnitBehavior::Exception(code) => ServiceOutcome::Exception(code),
            }
            .into_transport_disposition(function_code, is_broadcast),
        };
    };

    let (outcome, timed_out) = if let Some(timeout) = policy.request_timeout {
        match tokio::time::timeout(timeout, async { service.call_view(service_request) }).await {
            Ok(outcome) => (outcome, false),
            Err(_) => (ServiceOutcome::Exception(ExceptionCode::SlaveDeviceBusy), true),
        }
    } else {
        (service.call_view(service_request), false)
    };

    ExecutedTransportRequest {
        function_code,
        unit_id,
        timed_out,
        disposition: outcome.into_transport_disposition(function_code, is_broadcast),
    }
}

/// Default service backed by the standard Modbus semantic core and an optional
/// compatibility registry for custom function handlers.
#[derive(Clone, Default)]
pub struct StandardModbusService {
    custom_extensions: Option<Arc<ExtensionRegistry>>,
}

impl StandardModbusService {
    pub fn new(handlers: HandlerRegistry) -> Self {
        Self {
            custom_extensions: Some(Arc::new(ExtensionRegistry::from(handlers))),
        }
    }

    pub fn with_extensions(extensions: ExtensionRegistry) -> Self {
        Self {
            custom_extensions: Some(Arc::new(extensions)),
        }
    }

    fn execute_builtin_unicast(
        &self,
        request: &SemanticRequest,
        target: &RequestTarget,
    ) -> Result<SemanticResponse, ExceptionCode> {
        execute_builtin(request, target.address_space().as_ref())
    }

    fn execute_builtin_broadcast(
        &self,
        request: &SemanticRequest,
        targets: &[RequestTarget],
    ) -> Result<SemanticResponse, ExceptionCode> {
        for target in targets {
            execute_builtin_write_only(request, target.address_space().as_ref())?;
        }

        Ok(match request {
            SemanticRequest::WriteSingleCoil { address, value } => {
                SemanticResponse::WriteSingleCoilAck {
                    address: *address,
                    value: *value,
                }
            }
            SemanticRequest::WriteSingleRegister { address, value } => {
                SemanticResponse::WriteSingleRegisterAck {
                    address: *address,
                    value: *value,
                }
            }
            SemanticRequest::WriteMultipleCoils { address, values } => {
                SemanticResponse::WriteMultipleAck {
                    function: FunctionCode::WriteMultipleCoils,
                    address: *address,
                    quantity: values.len() as u16,
                }
            }
            SemanticRequest::WriteMultipleRegisters { address, values } => {
                SemanticResponse::WriteMultipleAck {
                    function: FunctionCode::WriteMultipleRegisters,
                    address: *address,
                    quantity: values.len() as u16,
                }
            }
            SemanticRequest::MaskWriteRegister {
                address,
                and_mask,
                or_mask,
            } => SemanticResponse::MaskWriteAck {
                address: *address,
                and_mask: *and_mask,
                or_mask: *or_mask,
            },
            _ => return Err(ExceptionCode::IllegalFunction),
        })
    }

    fn dispatch_custom_raw(
        &self,
        unit_id: u8,
        transaction_id: u16,
        raw_pdu: &[u8],
        targets: ServiceTargetsRef<'_>,
    ) -> ServiceOutcome {
        let extensions = match self.custom_extensions.as_ref() {
            Some(extensions) => extensions,
            None => return ServiceOutcome::Exception(ExceptionCode::IllegalFunction),
        };

        extensions.dispatch(unit_id, transaction_id, raw_pdu, targets)
    }

    fn dispatch_request(
        &self,
        unit_id: u8,
        transaction_id: u16,
        raw_pdu: &[u8],
        targets: ServiceTargetsRef<'_>,
    ) -> ServiceOutcome {
        let function_code = match raw_pdu.first().copied() {
            Some(code) => code,
            None => return ServiceOutcome::Exception(ExceptionCode::IllegalDataValue),
        };
        let is_broadcast = matches!(targets, ServiceTargetsRef::Broadcast(_));

        if FunctionCode::try_from(function_code).is_err() {
            return self.dispatch_custom_raw(unit_id, transaction_id, raw_pdu, targets);
        }

        let semantic = match parse_semantic_request(raw_pdu, is_broadcast) {
            Ok(semantic) => semantic,
            Err(error) => return ServiceOutcome::Exception(error),
        };
        let response = if is_broadcast {
            match self.execute_builtin_broadcast(
                &semantic,
                match targets {
                    ServiceTargetsRef::Broadcast(targets) => targets,
                    ServiceTargetsRef::Unicast(_) => &[],
                },
            ) {
                Ok(response) => response,
                Err(error) => return ServiceOutcome::Exception(error),
            }
        } else {
            let target = match targets {
                ServiceTargetsRef::Unicast(target) => target,
                ServiceTargetsRef::Broadcast(_) => unreachable!("broadcast handled separately"),
            };
            match self.execute_builtin_unicast(&semantic, target) {
                Ok(response) => response,
                Err(error) => return ServiceOutcome::Exception(error),
            }
        };

        match response.encode() {
            Ok(response) => ServiceOutcome::Reply(response),
            Err(_) => ServiceOutcome::Exception(ExceptionCode::SlaveDeviceFailure),
        }
    }
}

impl ModbusService for StandardModbusService {
    fn call(&self, request: &ServiceRequest) -> ServiceOutcome {
        let targets = match &request.targets {
            ServiceTargets::Unicast(target) => ServiceTargetsRef::Unicast(target),
            ServiceTargets::Broadcast(targets) => ServiceTargetsRef::Broadcast(targets),
        };

        self.dispatch_request(
            request.unit_id(),
            request.transaction_id(),
            request.pdu().as_bytes(),
            targets,
        )
    }

    fn call_view(&self, request: ServiceRequestView<'_>) -> ServiceOutcome {
        let targets = match request.targets {
            ServiceTargetsRef::Unicast(target) => ServiceTargetsRef::Unicast(target),
            ServiceTargetsRef::Broadcast(targets) => ServiceTargetsRef::Broadcast(targets),
        };

        self.dispatch_request(
            request.unit_id(),
            request.transaction_id(),
            request.pdu(),
            targets,
        )
    }
}

fn execute_builtin(
    request: &SemanticRequest,
    address_space: &dyn AddressSpace,
) -> Result<SemanticResponse, ExceptionCode> {
    match request {
        SemanticRequest::ReadCoils { address, quantity } => address_space
            .read_coils(*address, *quantity)
            .map(|values| SemanticResponse::Bits {
                function: FunctionCode::ReadCoils,
                values,
            })
            .map_err(map_modbus_error),
        SemanticRequest::ReadDiscreteInputs { address, quantity } => address_space
            .read_discrete_inputs(*address, *quantity)
            .map(|values| SemanticResponse::Bits {
                function: FunctionCode::ReadDiscreteInputs,
                values,
            })
            .map_err(map_modbus_error),
        SemanticRequest::ReadHoldingRegisters { address, quantity } => address_space
            .read_holding_registers(*address, *quantity)
            .map(|values| SemanticResponse::Registers {
                function: FunctionCode::ReadHoldingRegisters,
                values,
            })
            .map_err(map_modbus_error),
        SemanticRequest::ReadInputRegisters { address, quantity } => address_space
            .read_input_registers(*address, *quantity)
            .map(|values| SemanticResponse::Registers {
                function: FunctionCode::ReadInputRegisters,
                values,
            })
            .map_err(map_modbus_error),
        SemanticRequest::WriteSingleCoil { address, value } => {
            address_space
                .write_coil(*address, *value)
                .map_err(map_modbus_error)?;
            Ok(SemanticResponse::WriteSingleCoilAck {
                address: *address,
                value: *value,
            })
        }
        SemanticRequest::WriteSingleRegister { address, value } => {
            address_space
                .write_holding_register(*address, *value)
                .map_err(map_modbus_error)?;
            Ok(SemanticResponse::WriteSingleRegisterAck {
                address: *address,
                value: *value,
            })
        }
        SemanticRequest::WriteMultipleCoils { address, values } => {
            address_space
                .write_coils(*address, values)
                .map_err(map_modbus_error)?;
            Ok(SemanticResponse::WriteMultipleAck {
                function: FunctionCode::WriteMultipleCoils,
                address: *address,
                quantity: values.len() as u16,
            })
        }
        SemanticRequest::WriteMultipleRegisters { address, values } => {
            address_space
                .write_holding_registers(*address, values)
                .map_err(map_modbus_error)?;
            Ok(SemanticResponse::WriteMultipleAck {
                function: FunctionCode::WriteMultipleRegisters,
                address: *address,
                quantity: values.len() as u16,
            })
        }
        SemanticRequest::MaskWriteRegister {
            address,
            and_mask,
            or_mask,
        } => {
            address_space
                .mask_write_holding_register(*address, *and_mask, *or_mask)
                .map_err(map_modbus_error)?;
            Ok(SemanticResponse::MaskWriteAck {
                address: *address,
                and_mask: *and_mask,
                or_mask: *or_mask,
            })
        }
        SemanticRequest::ReadWriteMultipleRegisters {
            read_address,
            read_quantity,
            write_address,
            values,
        } => {
            address_space
                .write_holding_registers(*write_address, values)
                .map_err(map_modbus_error)?;
            let registers = address_space
                .read_holding_registers(*read_address, *read_quantity)
                .map_err(map_modbus_error)?;
            Ok(SemanticResponse::Registers {
                function: FunctionCode::ReadWriteMultipleRegisters,
                values: registers,
            })
        }
        SemanticRequest::Custom { function_code, .. } => {
            let _ = function_code;
            Err(ExceptionCode::IllegalFunction)
        }
    }
}

fn execute_builtin_write_only(
    request: &SemanticRequest,
    address_space: &dyn AddressSpace,
) -> Result<(), ExceptionCode> {
    match request {
        SemanticRequest::WriteSingleCoil { address, value } => address_space
            .write_coil(*address, *value)
            .map_err(map_modbus_error),
        SemanticRequest::WriteSingleRegister { address, value } => address_space
            .write_holding_register(*address, *value)
            .map_err(map_modbus_error),
        SemanticRequest::WriteMultipleCoils { address, values } => address_space
            .write_coils(*address, values)
            .map_err(map_modbus_error),
        SemanticRequest::WriteMultipleRegisters { address, values } => address_space
            .write_holding_registers(*address, values)
            .map_err(map_modbus_error),
        SemanticRequest::MaskWriteRegister {
            address,
            and_mask,
            or_mask,
        } => address_space
            .mask_write_holding_register(*address, *and_mask, *or_mask)
            .map_err(map_modbus_error),
        _ => Err(ExceptionCode::IllegalFunction),
    }
}

fn map_modbus_error(error: ModbusError) -> ExceptionCode {
    match error {
        ModbusError::InvalidAddress { .. } | ModbusError::DeviceNotFound { .. } => {
            ExceptionCode::IllegalDataAddress
        }
        ModbusError::InvalidQuantity { .. } => ExceptionCode::IllegalDataAddress,
        ModbusError::InvalidData(_) => ExceptionCode::IllegalDataValue,
        _ => ExceptionCode::SlaveDeviceFailure,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::context::ServerContext;
    use crate::register::RegisterStore;

    use super::{
        execute_transport_request, StandardModbusService, TransportDisposition,
        TransportServicePolicy, UnknownUnitBehavior,
    };

    #[tokio::test]
    async fn shared_transport_request_returns_exception_for_unknown_tcp_unit() {
        let service = StandardModbusService::default();
        let context = ServerContext::new(Arc::new(RegisterStore::new(16, 16, 16, 16)));
        let result = execute_transport_request(
            &service,
            &context,
            42,
            7,
            &[0x03, 0x00, 0x00, 0x00, 0x01],
            TransportServicePolicy::new(UnknownUnitBehavior::Exception(
                crate::handler::ExceptionCode::GatewayTargetDeviceFailedToRespond,
            )),
        )
        .await;

        match result.disposition {
            TransportDisposition::Reply(response) => {
                assert!(response.is_exception());
            }
            other => panic!("unexpected disposition: {:?}", other),
        }
    }

    #[tokio::test]
    async fn shared_transport_request_suppresses_broadcast_reply() {
        let service = StandardModbusService::default();
        let context = ServerContext::new(Arc::new(RegisterStore::new(16, 16, 16, 16)));
        let result = execute_transport_request(
            &service,
            &context,
            0,
            0,
            &[0x05, 0x00, 0x00, 0xFF, 0x00],
            TransportServicePolicy::new(UnknownUnitBehavior::Ignore)
                .with_request_timeout(Some(Duration::from_millis(5))),
        )
        .await;

        assert!(matches!(
            result.disposition,
            TransportDisposition::BroadcastSuppressed(_)
        ));
    }
}
