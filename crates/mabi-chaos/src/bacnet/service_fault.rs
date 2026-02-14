//! BACnet service-level fault injection.
//!
//! Simulates BACnet service failures at the application layer:
//! - Return BACnet REJECT PDU for specific services
//! - Return BACnet ABORT PDU for specific services
//! - Return BACnet ERROR PDU with configurable class/code
//! - Drop (no response) for specific services
//! - Return wrong service type in response

use std::any::Any;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::context::FaultContext;
use crate::error::ChaosResult;
use crate::fault::{
    BaseFault, Fault, FaultBehavior, FaultCategory, FaultMetadata, FaultSeverity, FaultStatistics,
};

// =============================================================================
// Service Fault Type
// =============================================================================

/// Type of service-level fault to inject.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceFaultType {
    /// Return a BACnet Reject PDU.
    RejectService {
        /// Reject reason code (per ASHRAE 135, Table 21-2).
        /// 0=Other, 1=BufferOverflow, 2=InconsistentParameters,
        /// 3=InvalidParameterDataType, 4=InvalidTag,
        /// 5=MissingRequiredParameter, 6=ParameterOutOfRange,
        /// 7=TooManyArguments, 8=UndefinedEnumeration, 9=UnrecognizedService.
        reject_reason: u8,
    },

    /// Return a BACnet Abort PDU.
    AbortService {
        /// Abort reason code (per ASHRAE 135, Table 21-1).
        /// 0=Other, 1=BufferOverflow, 2=InvalidApduInThisState,
        /// 3=PreemptedByHigherPriority, 4=SegmentationNotSupported,
        /// 5=SecurityError, 6=InsufficientSecurity, 7=WindowSizeOutOfRange,
        /// 8=ApplicationExceededReplyTime, 9=OutOfResources,
        /// 10=TsmTimeout, 11=ApduTooLong.
        abort_reason: u8,
        /// True if server-initiated abort.
        server: bool,
    },

    /// Return a BACnet Error PDU.
    ErrorResponse {
        /// Error class (per ASHRAE 135, Table 18-1).
        /// 0=Device, 1=Object, 2=Property, 3=Resources, 4=Security, 5=Services.
        error_class: u32,
        /// Error code (per ASHRAE 135, Table 18-2).
        error_code: u32,
    },

    /// Drop the request entirely (no response, triggering client timeout).
    DropRequest,

    /// Return a response for a different (wrong) service type.
    WrongServiceResponse {
        /// The wrong service choice to return.
        wrong_service_choice: u8,
    },

    /// Return a SimpleAck when a ComplexAck is expected.
    SimpleAckInsteadOfComplex,

    /// Return a ComplexAck with empty payload.
    EmptyComplexAck,
}

impl Default for ServiceFaultType {
    fn default() -> Self {
        Self::RejectService { reject_reason: 9 }
    }
}

// =============================================================================
// Service Fault Configuration
// =============================================================================

/// Configuration for service-level fault injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceFaultConfig {
    /// Type of service fault.
    pub fault_type: ServiceFaultType,

    /// BACnet confirmed service choice codes to target.
    /// Empty = target all services.
    #[serde(default)]
    pub target_services: Vec<u8>,

    /// Whether to also match unconfirmed services.
    #[serde(default)]
    pub include_unconfirmed: bool,
}

impl Default for ServiceFaultConfig {
    fn default() -> Self {
        Self {
            fault_type: ServiceFaultType::RejectService { reject_reason: 9 },
            target_services: Vec::new(),
            include_unconfirmed: false,
        }
    }
}

impl ServiceFaultConfig {
    /// Reject specific services with the given reason.
    pub fn reject(reject_reason: u8) -> Self {
        Self {
            fault_type: ServiceFaultType::RejectService { reject_reason },
            ..Default::default()
        }
    }

    /// Abort specific services.
    pub fn abort(abort_reason: u8, server: bool) -> Self {
        Self {
            fault_type: ServiceFaultType::AbortService {
                abort_reason,
                server,
            },
            ..Default::default()
        }
    }

    /// Return error for specific services.
    pub fn error(error_class: u32, error_code: u32) -> Self {
        Self {
            fault_type: ServiceFaultType::ErrorResponse {
                error_class,
                error_code,
            },
            ..Default::default()
        }
    }

    /// Drop requests entirely.
    pub fn drop_request() -> Self {
        Self {
            fault_type: ServiceFaultType::DropRequest,
            ..Default::default()
        }
    }

    /// Only target specific service choice codes.
    pub fn for_services(mut self, services: Vec<u8>) -> Self {
        self.target_services = services;
        self
    }

    /// Also affect unconfirmed services.
    pub fn with_unconfirmed(mut self) -> Self {
        self.include_unconfirmed = true;
        self
    }
}

// =============================================================================
// Service Fault Implementation
// =============================================================================

/// BACnet service-level fault.
///
/// Simulates BACnet service failures by returning error responses,
/// rejecting or aborting specific services, or dropping requests entirely.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::bacnet::ServiceFault;
///
/// // Reject ReadPropertyMultiple with "unrecognized service"
/// let fault = ServiceFault::builder()
///     .id("svc-reject-001")
///     .reject(9) // UnrecognizedService
///     .for_services(vec![14]) // ReadPropertyMultiple
///     .probability(0.2)
///     .target("bacnet-*")
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct ServiceFault {
    base: BaseFault,
    config: ServiceFaultConfig,
}

impl ServiceFault {
    pub fn new(id: impl Into<String>, config: ServiceFaultConfig) -> Self {
        let id = id.into();
        let metadata =
            FaultMetadata::new(&id, "BACnet Service Fault", FaultCategory::Protocol)
                .with_description("Injects BACnet service-level failures")
                .with_severity(FaultSeverity::Medium)
                .with_tag("bacnet")
                .with_tag("service");

        Self {
            base: BaseFault::new(metadata),
            config,
        }
    }

    pub fn builder() -> ServiceFaultBuilder {
        ServiceFaultBuilder::default()
    }

    /// Check if a service choice code matches the filter.
    pub fn matches_service(&self, service_choice: u8) -> bool {
        if self.config.target_services.is_empty() {
            return true; // No filter = match all
        }
        self.config.target_services.contains(&service_choice)
    }

    /// Get the FaultBehavior for this fault type.
    pub fn behavior(&self) -> FaultBehavior {
        match &self.config.fault_type {
            ServiceFaultType::RejectService { reject_reason } => FaultBehavior::ReturnError {
                error_code: *reject_reason as i32,
                message: format!("BACnet REJECT: reason {}", reject_reason),
            },
            ServiceFaultType::AbortService { abort_reason, .. } => FaultBehavior::Abort {
                error: format!("BACnet ABORT: reason {}", abort_reason),
            },
            ServiceFaultType::ErrorResponse {
                error_class,
                error_code,
            } => FaultBehavior::ReturnError {
                error_code: *error_code as i32,
                message: format!(
                    "BACnet ERROR: class={}, code={}",
                    error_class, error_code
                ),
            },
            ServiceFaultType::DropRequest => FaultBehavior::Skip,
            ServiceFaultType::WrongServiceResponse { .. } => FaultBehavior::Modify,
            ServiceFaultType::SimpleAckInsteadOfComplex => FaultBehavior::Modify,
            ServiceFaultType::EmptyComplexAck => FaultBehavior::Modify,
        }
    }

    pub fn config(&self) -> &ServiceFaultConfig {
        &self.config
    }
}

#[async_trait]
impl Fault for ServiceFault {
    fn metadata(&self) -> &FaultMetadata {
        &self.base.metadata
    }

    fn enable(&mut self) {
        self.base.enable();
    }

    fn disable(&mut self) {
        self.base.disable();
    }

    async fn should_activate(&self, ctx: &FaultContext) -> ChaosResult<bool> {
        if !self.base.matches_target(ctx.target.identifier()) {
            return Ok(false);
        }

        // Check service filter via metadata
        if let Some(svc_str) = ctx.get_metadata("bacnet_service_choice") {
            if let Ok(svc) = svc_str.parse::<u8>() {
                if !self.matches_service(svc) {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    async fn apply(&self, ctx: &mut FaultContext) -> ChaosResult<FaultBehavior> {
        if !self.clone().base.check_probability() {
            return Ok(FaultBehavior::Continue);
        }

        ctx.set_metadata("bacnet_service_fault", &format!("{:?}", self.config.fault_type));
        ctx.record_applied_fault(self.id(), "service_fault");

        tracing::debug!(
            fault_id = %self.id(),
            target = %ctx.target.device_id,
            fault_type = ?self.config.fault_type,
            "Applying BACnet service fault"
        );

        Ok(self.behavior())
    }

    fn reset(&mut self) {
        self.base.reset();
    }

    fn statistics(&self) -> FaultStatistics {
        self.base.statistics.clone()
    }

    fn clone_box(&self) -> Box<dyn Fault> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// =============================================================================
// Builder
// =============================================================================

#[derive(Debug, Default)]
pub struct ServiceFaultBuilder {
    id: Option<String>,
    name: Option<String>,
    config: ServiceFaultConfig,
    probability: f64,
    targets: Vec<String>,
    severity: FaultSeverity,
}

impl ServiceFaultBuilder {
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn fault_type(mut self, t: ServiceFaultType) -> Self {
        self.config.fault_type = t;
        self
    }

    pub fn reject(mut self, reject_reason: u8) -> Self {
        self.config.fault_type = ServiceFaultType::RejectService { reject_reason };
        self
    }

    pub fn abort(mut self, abort_reason: u8, server: bool) -> Self {
        self.config.fault_type = ServiceFaultType::AbortService {
            abort_reason,
            server,
        };
        self
    }

    pub fn error(mut self, error_class: u32, error_code: u32) -> Self {
        self.config.fault_type = ServiceFaultType::ErrorResponse {
            error_class,
            error_code,
        };
        self
    }

    pub fn drop_request(mut self) -> Self {
        self.config.fault_type = ServiceFaultType::DropRequest;
        self
    }

    pub fn wrong_service(mut self, wrong_choice: u8) -> Self {
        self.config.fault_type = ServiceFaultType::WrongServiceResponse {
            wrong_service_choice: wrong_choice,
        };
        self
    }

    pub fn for_services(mut self, services: Vec<u8>) -> Self {
        self.config.target_services = services;
        self
    }

    pub fn with_unconfirmed(mut self) -> Self {
        self.config.include_unconfirmed = true;
        self
    }

    pub fn probability(mut self, p: f64) -> Self {
        self.probability = p.clamp(0.0, 1.0);
        self
    }

    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.targets.push(target.into());
        self
    }

    pub fn severity(mut self, severity: FaultSeverity) -> Self {
        self.severity = severity;
        self
    }

    pub fn build(self) -> ServiceFault {
        let id = self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut fault = ServiceFault::new(&id, self.config);

        if let Some(name) = self.name {
            fault.base.metadata.name = name;
        }

        fault.base.metadata.probability = if self.probability == 0.0 {
            1.0
        } else {
            self.probability
        };
        fault.base.metadata.targets = self.targets;
        fault.base.metadata.severity = self.severity;

        fault
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reject_behavior() {
        let config = ServiceFaultConfig::reject(9);
        let fault = ServiceFault::new("test", config);

        match fault.behavior() {
            FaultBehavior::ReturnError { error_code, message } => {
                assert_eq!(error_code, 9);
                assert!(message.contains("REJECT"));
            }
            other => panic!("Expected ReturnError, got {:?}", other),
        }
    }

    #[test]
    fn test_abort_behavior() {
        let config = ServiceFaultConfig::abort(4, true); // SegmentationNotSupported
        let fault = ServiceFault::new("test", config);

        match fault.behavior() {
            FaultBehavior::Abort { error } => {
                assert!(error.contains("ABORT"));
                assert!(error.contains("4"));
            }
            other => panic!("Expected Abort, got {:?}", other),
        }
    }

    #[test]
    fn test_error_behavior() {
        let config = ServiceFaultConfig::error(1, 31); // Object, UnknownObject
        let fault = ServiceFault::new("test", config);

        match fault.behavior() {
            FaultBehavior::ReturnError { error_code, message } => {
                assert_eq!(error_code, 31);
                assert!(message.contains("ERROR"));
            }
            other => panic!("Expected ReturnError, got {:?}", other),
        }
    }

    #[test]
    fn test_drop_behavior() {
        let config = ServiceFaultConfig::drop_request();
        let fault = ServiceFault::new("test", config);
        assert_eq!(fault.behavior(), FaultBehavior::Skip);
    }

    #[test]
    fn test_service_filter() {
        let config = ServiceFaultConfig::reject(9).for_services(vec![12, 14, 15]);
        let fault = ServiceFault::new("test", config);

        assert!(fault.matches_service(12));  // ReadProperty
        assert!(fault.matches_service(14));  // ReadPropertyMultiple
        assert!(!fault.matches_service(6));  // WriteProperty
    }

    #[test]
    fn test_empty_filter_matches_all() {
        let config = ServiceFaultConfig::reject(9);
        let fault = ServiceFault::new("test", config);

        assert!(fault.matches_service(0));
        assert!(fault.matches_service(12));
        assert!(fault.matches_service(255));
    }

    #[test]
    fn test_builder() {
        let fault = ServiceFault::builder()
            .id("svc-001")
            .reject(5) // MissingRequiredParameter
            .for_services(vec![12])
            .probability(0.3)
            .target("bacnet-*")
            .build();

        assert_eq!(fault.id(), "svc-001");
        assert!(fault.matches_service(12));
        assert!(!fault.matches_service(14));
    }
}
