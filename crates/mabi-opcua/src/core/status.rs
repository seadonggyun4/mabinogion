use thiserror::Error;

use crate::error::OpcUaError;
use crate::types::{NodeId, StatusCode};

#[derive(Debug, Error)]
pub(crate) enum ServiceError {
    #[error(transparent)]
    OpcUa(#[from] OpcUaError),

    #[error("Unsupported built-in service: {type_id}")]
    UnsupportedService { type_id: NodeId },
}

impl ServiceError {
    pub(crate) fn status_code(&self) -> StatusCode {
        match self {
            ServiceError::UnsupportedService { .. } => StatusCode::BAD_SERVICE_UNSUPPORTED,
            ServiceError::OpcUa(error) => match error {
                OpcUaError::ServiceNotSupported { .. } => StatusCode::BAD_SERVICE_UNSUPPORTED,
                OpcUaError::InvalidState(_) => StatusCode::BAD_SESSION_ID_INVALID,
                OpcUaError::BadSecureChannelId(_) => StatusCode::BAD_SECURITY_CHECKS_FAILED,
                OpcUaError::BadSequenceNumber { .. } => StatusCode::BAD_SECURITY_CHECKS_FAILED,
                OpcUaError::MessageTooLarge { .. } => StatusCode::BAD_REQUEST_TIMEOUT,
                OpcUaError::Security(_) => StatusCode::BAD_SECURITY_CHECKS_FAILED,
                OpcUaError::NodeNotFound { .. } | OpcUaError::InvalidNodeId(_) => {
                    StatusCode::BAD_NODE_ID_UNKNOWN
                }
                _ => StatusCode::BAD_INTERNAL_ERROR,
            },
        }
    }
}

impl From<ServiceError> for OpcUaError {
    fn from(value: ServiceError) -> Self {
        match value {
            ServiceError::OpcUa(error) => error,
            ServiceError::UnsupportedService { type_id } => OpcUaError::ServiceNotSupported {
                service_id: type_id.to_string(),
            },
        }
    }
}
