//! Shared transport request runtime used by both TCP and RTU adapters.

use std::time::Duration;

use crate::context::ServerContext;
use crate::core::{build_exception_pdu, ExceptionCode, ResponsePdu};
use crate::service::{ModbusService, ServiceOutcome, ServiceRequestView};

/// Cached hook bundle shared by transport request loops.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TransportHookBundle {
    pub request_timeout: Option<Duration>,
    pub record_transport_metrics: bool,
}

impl TransportHookBundle {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_request_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.request_timeout = timeout;
        self
    }

    pub(crate) fn with_transport_metrics(mut self, enabled: bool) -> Self {
        self.record_transport_metrics = enabled;
        self
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

    pub(crate) fn summary(&self) -> TransportOutcomeSummary {
        match &self.disposition {
            TransportDisposition::Reply(response) => TransportOutcomeSummary {
                kind: TransportOutcomeKind::Reply,
                is_exception: response.is_exception(),
            },
            TransportDisposition::BroadcastSuppressed(response) => TransportOutcomeSummary {
                kind: TransportOutcomeKind::BroadcastSuppressed,
                is_exception: response.is_exception(),
            },
            TransportDisposition::Ignore => TransportOutcomeSummary {
                kind: TransportOutcomeKind::Ignore,
                is_exception: false,
            },
        }
    }

    pub fn response_pdu(&self) -> Option<&ResponsePdu> {
        match &self.disposition {
            TransportDisposition::Reply(response)
            | TransportDisposition::BroadcastSuppressed(response) => Some(response),
            TransportDisposition::Ignore => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransportOutcomeKind {
    Reply,
    BroadcastSuppressed,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TransportOutcomeSummary {
    pub kind: TransportOutcomeKind,
    pub is_exception: bool,
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
            Err(_) => (
                ServiceOutcome::Exception(ExceptionCode::SlaveDeviceBusy),
                true,
            ),
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

pub(crate) fn exception_response(function_code: u8, code: ExceptionCode) -> ResponsePdu {
    ResponsePdu::new(build_exception_pdu(function_code, code))
        .expect("exception PDUs are always valid")
}
