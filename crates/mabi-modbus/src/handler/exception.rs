//! Modbus exception codes and responses.
//!
//! This module defines all standard Modbus exception codes as per
//! the Modbus Application Protocol Specification V1.1b3.

use std::fmt;

/// Modbus exception codes.
///
/// These codes are returned when a Modbus slave cannot process a request.
/// The response PDU will have the function code OR'd with 0x80, followed
/// by the exception code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ExceptionCode {
    /// 0x01: The function code received in the query is not recognized
    /// or allowed by the server.
    IllegalFunction = 0x01,

    /// 0x02: The data address received in the query is not an allowable
    /// address for the server.
    IllegalDataAddress = 0x02,

    /// 0x03: A value contained in the query data field is not an allowable
    /// value for the server.
    IllegalDataValue = 0x03,

    /// 0x04: An unrecoverable error occurred while the server was attempting
    /// to perform the requested action.
    SlaveDeviceFailure = 0x04,

    /// 0x05: The server has accepted the request and is processing it,
    /// but a long duration of time will be required to do so.
    Acknowledge = 0x05,

    /// 0x06: The server is engaged in processing a long-duration program command.
    SlaveDeviceBusy = 0x06,

    /// 0x08: Specialized use in conjunction with programming commands.
    /// The server detected a parity error in the memory.
    MemoryParityError = 0x08,

    /// 0x0A: Specialized use in conjunction with gateways.
    /// The gateway was unable to allocate an internal communication path.
    GatewayPathUnavailable = 0x0A,

    /// 0x0B: Specialized use in conjunction with gateways.
    /// No response was obtained from the target device.
    GatewayTargetDeviceFailedToRespond = 0x0B,
}

impl ExceptionCode {
    /// Convert from u8 to ExceptionCode.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::IllegalFunction),
            0x02 => Some(Self::IllegalDataAddress),
            0x03 => Some(Self::IllegalDataValue),
            0x04 => Some(Self::SlaveDeviceFailure),
            0x05 => Some(Self::Acknowledge),
            0x06 => Some(Self::SlaveDeviceBusy),
            0x08 => Some(Self::MemoryParityError),
            0x0A => Some(Self::GatewayPathUnavailable),
            0x0B => Some(Self::GatewayTargetDeviceFailedToRespond),
            _ => None,
        }
    }

    /// Get the numeric value of this exception code.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Returns a human-readable description of this exception.
    pub fn description(&self) -> &'static str {
        match self {
            Self::IllegalFunction => "The function code is not recognized or allowed",
            Self::IllegalDataAddress => "The data address is not allowable",
            Self::IllegalDataValue => "The data value is not allowable",
            Self::SlaveDeviceFailure => "An unrecoverable error occurred",
            Self::Acknowledge => "Request accepted, processing in progress",
            Self::SlaveDeviceBusy => "Server is busy processing another request",
            Self::MemoryParityError => "Memory parity error detected",
            Self::GatewayPathUnavailable => "Gateway path unavailable",
            Self::GatewayTargetDeviceFailedToRespond => "Target device failed to respond",
        }
    }
}

impl fmt::Display for ExceptionCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02X}: {}", self.as_u8(), self.description())
    }
}

impl From<ExceptionCode> for u8 {
    fn from(code: ExceptionCode) -> Self {
        code as u8
    }
}

/// Represents a complete Modbus exception response.
#[derive(Debug, Clone)]
pub struct ExceptionResponse {
    /// The original function code (without 0x80 flag).
    pub function_code: u8,

    /// The exception code.
    pub exception_code: ExceptionCode,
}

impl ExceptionResponse {
    /// Create a new exception response.
    pub fn new(function_code: u8, exception_code: ExceptionCode) -> Self {
        Self {
            function_code,
            exception_code,
        }
    }

    /// Build the exception response PDU.
    ///
    /// Returns a 2-byte PDU: [function_code | 0x80, exception_code]
    pub fn to_pdu(&self) -> Vec<u8> {
        vec![self.function_code | 0x80, self.exception_code.as_u8()]
    }

    /// Parse an exception response from a PDU.
    pub fn from_pdu(pdu: &[u8]) -> Option<Self> {
        if pdu.len() < 2 {
            return None;
        }

        // Check if this is an exception (function code has 0x80 flag)
        if pdu[0] & 0x80 == 0 {
            return None;
        }

        let function_code = pdu[0] & 0x7F;
        let exception_code = ExceptionCode::from_u8(pdu[1])?;

        Some(Self {
            function_code,
            exception_code,
        })
    }

    /// Check if a PDU is an exception response.
    pub fn is_exception(pdu: &[u8]) -> bool {
        !pdu.is_empty() && (pdu[0] & 0x80) != 0
    }
}

/// Helper function to build an exception response PDU.
pub fn build_exception_pdu(function_code: u8, exception_code: ExceptionCode) -> Vec<u8> {
    ExceptionResponse::new(function_code, exception_code).to_pdu()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exception_code_values() {
        assert_eq!(ExceptionCode::IllegalFunction as u8, 0x01);
        assert_eq!(ExceptionCode::IllegalDataAddress as u8, 0x02);
        assert_eq!(ExceptionCode::IllegalDataValue as u8, 0x03);
        assert_eq!(ExceptionCode::SlaveDeviceFailure as u8, 0x04);
        assert_eq!(
            ExceptionCode::GatewayTargetDeviceFailedToRespond as u8,
            0x0B
        );
    }

    #[test]
    fn test_exception_code_from_u8() {
        assert_eq!(
            ExceptionCode::from_u8(0x01),
            Some(ExceptionCode::IllegalFunction)
        );
        assert_eq!(
            ExceptionCode::from_u8(0x02),
            Some(ExceptionCode::IllegalDataAddress)
        );
        assert_eq!(ExceptionCode::from_u8(0xFF), None);
    }

    #[test]
    fn test_exception_response_to_pdu() {
        let response = ExceptionResponse::new(0x03, ExceptionCode::IllegalDataAddress);
        let pdu = response.to_pdu();

        assert_eq!(pdu, vec![0x83, 0x02]);
    }

    #[test]
    fn test_exception_response_from_pdu() {
        let pdu = [0x83, 0x02];
        let response = ExceptionResponse::from_pdu(&pdu).unwrap();

        assert_eq!(response.function_code, 0x03);
        assert_eq!(response.exception_code, ExceptionCode::IllegalDataAddress);
    }

    #[test]
    fn test_is_exception() {
        assert!(ExceptionResponse::is_exception(&[0x83, 0x02]));
        assert!(!ExceptionResponse::is_exception(&[0x03, 0x02]));
        assert!(!ExceptionResponse::is_exception(&[]));
    }
}
