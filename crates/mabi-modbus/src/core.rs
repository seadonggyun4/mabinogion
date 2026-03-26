//! Protocol-level Modbus core types and semantic request parsing.

use crate::error::{ModbusError, ModbusResult};
pub use crate::handler::{build_exception_pdu, ExceptionCode, ExceptionResponse};

/// Supported Modbus function codes handled by the simulator core.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionCode {
    ReadCoils = 0x01,
    ReadDiscreteInputs = 0x02,
    ReadHoldingRegisters = 0x03,
    ReadInputRegisters = 0x04,
    WriteSingleCoil = 0x05,
    WriteSingleRegister = 0x06,
    WriteMultipleCoils = 0x0F,
    WriteMultipleRegisters = 0x10,
    MaskWriteRegister = 0x16,
    ReadWriteMultipleRegisters = 0x17,
}

impl TryFrom<u8> for FunctionCode {
    type Error = ModbusError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Self::ReadCoils),
            0x02 => Ok(Self::ReadDiscreteInputs),
            0x03 => Ok(Self::ReadHoldingRegisters),
            0x04 => Ok(Self::ReadInputRegisters),
            0x05 => Ok(Self::WriteSingleCoil),
            0x06 => Ok(Self::WriteSingleRegister),
            0x0F => Ok(Self::WriteMultipleCoils),
            0x10 => Ok(Self::WriteMultipleRegisters),
            0x16 => Ok(Self::MaskWriteRegister),
            0x17 => Ok(Self::ReadWriteMultipleRegisters),
            _ => Err(ModbusError::InvalidFunction(value)),
        }
    }
}

impl From<FunctionCode> for u8 {
    fn from(value: FunctionCode) -> Self {
        value as u8
    }
}

/// A validated Modbus Protocol Data Unit request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestPdu {
    bytes: Vec<u8>,
}

impl RequestPdu {
    /// Create a validated request PDU.
    pub fn new(bytes: impl Into<Vec<u8>>) -> ModbusResult<Self> {
        let bytes = bytes.into();
        if bytes.is_empty() {
            return Err(ModbusError::InvalidData(
                "request PDU must contain at least a function code".to_string(),
            ));
        }
        Ok(Self { bytes })
    }

    /// Borrow the raw PDU bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consume the PDU into raw bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Read the raw function code byte.
    pub fn raw_function_code(&self) -> u8 {
        self.bytes[0]
    }

    /// Parse the function code.
    pub fn function_code(&self) -> ModbusResult<FunctionCode> {
        FunctionCode::try_from(self.raw_function_code())
    }

    /// Parse the PDU into the canonical typed semantic model.
    pub fn semantic_request(&self, is_broadcast: bool) -> Result<SemanticRequest, ExceptionCode> {
        parse_semantic_request(self.as_bytes(), is_broadcast)
    }
}

/// A validated Modbus response PDU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsePdu {
    bytes: Vec<u8>,
}

impl ResponsePdu {
    /// Create a validated response PDU.
    pub fn new(bytes: impl Into<Vec<u8>>) -> ModbusResult<Self> {
        let bytes = bytes.into();
        if bytes.is_empty() {
            return Err(ModbusError::InvalidData(
                "response PDU must contain at least a function code".to_string(),
            ));
        }
        Ok(Self { bytes })
    }

    /// Borrow the raw PDU bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consume the PDU into raw bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Check whether the response is an exception response.
    pub fn is_exception(&self) -> bool {
        ExceptionResponse::is_exception(self.as_bytes())
    }
}

/// Canonical typed semantic request model used by the built-in core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticRequest {
    ReadCoils {
        address: u16,
        quantity: u16,
    },
    ReadDiscreteInputs {
        address: u16,
        quantity: u16,
    },
    ReadHoldingRegisters {
        address: u16,
        quantity: u16,
    },
    ReadInputRegisters {
        address: u16,
        quantity: u16,
    },
    WriteSingleCoil {
        address: u16,
        value: bool,
    },
    WriteSingleRegister {
        address: u16,
        value: u16,
    },
    WriteMultipleCoils {
        address: u16,
        values: Vec<bool>,
    },
    WriteMultipleRegisters {
        address: u16,
        values: Vec<u16>,
    },
    MaskWriteRegister {
        address: u16,
        and_mask: u16,
        or_mask: u16,
    },
    ReadWriteMultipleRegisters {
        read_address: u16,
        read_quantity: u16,
        write_address: u16,
        values: Vec<u16>,
    },
    Custom {
        function_code: u8,
        payload: Vec<u8>,
    },
}

impl SemanticRequest {
    /// Returns the function code for the request.
    pub fn function_code(&self) -> u8 {
        match self {
            Self::ReadCoils { .. } => FunctionCode::ReadCoils as u8,
            Self::ReadDiscreteInputs { .. } => FunctionCode::ReadDiscreteInputs as u8,
            Self::ReadHoldingRegisters { .. } => FunctionCode::ReadHoldingRegisters as u8,
            Self::ReadInputRegisters { .. } => FunctionCode::ReadInputRegisters as u8,
            Self::WriteSingleCoil { .. } => FunctionCode::WriteSingleCoil as u8,
            Self::WriteSingleRegister { .. } => FunctionCode::WriteSingleRegister as u8,
            Self::WriteMultipleCoils { .. } => FunctionCode::WriteMultipleCoils as u8,
            Self::WriteMultipleRegisters { .. } => FunctionCode::WriteMultipleRegisters as u8,
            Self::MaskWriteRegister { .. } => FunctionCode::MaskWriteRegister as u8,
            Self::ReadWriteMultipleRegisters { .. } => {
                FunctionCode::ReadWriteMultipleRegisters as u8
            }
            Self::Custom { function_code, .. } => *function_code,
        }
    }

    /// Whether the function is broadcast-safe according to the Modbus spec.
    pub fn allows_broadcast(&self) -> bool {
        matches!(
            self,
            Self::WriteSingleCoil { .. }
                | Self::WriteSingleRegister { .. }
                | Self::WriteMultipleCoils { .. }
                | Self::WriteMultipleRegisters { .. }
                | Self::MaskWriteRegister { .. }
        )
    }
}

/// Canonical typed semantic response model used by the built-in core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticResponse {
    Bits {
        function: FunctionCode,
        values: Vec<bool>,
    },
    Registers {
        function: FunctionCode,
        values: Vec<u16>,
    },
    WriteSingleCoilAck {
        address: u16,
        value: bool,
    },
    WriteSingleRegisterAck {
        address: u16,
        value: u16,
    },
    WriteMultipleAck {
        function: FunctionCode,
        address: u16,
        quantity: u16,
    },
    MaskWriteAck {
        address: u16,
        and_mask: u16,
        or_mask: u16,
    },
    Custom(ResponsePdu),
}

/// Parse raw PDU bytes into the canonical typed semantic model.
pub fn parse_semantic_request(
    pdu: &[u8],
    is_broadcast: bool,
) -> Result<SemanticRequest, ExceptionCode> {
    let function_code = *pdu.first().ok_or(ExceptionCode::IllegalDataValue)?;
    let request = match FunctionCode::try_from(function_code) {
        Ok(FunctionCode::ReadCoils) => parse_read_bits(FunctionCode::ReadCoils, pdu)?,
        Ok(FunctionCode::ReadDiscreteInputs) => {
            parse_read_bits(FunctionCode::ReadDiscreteInputs, pdu)?
        }
        Ok(FunctionCode::ReadHoldingRegisters) => {
            parse_read_registers(FunctionCode::ReadHoldingRegisters, pdu)?
        }
        Ok(FunctionCode::ReadInputRegisters) => {
            parse_read_registers(FunctionCode::ReadInputRegisters, pdu)?
        }
        Ok(FunctionCode::WriteSingleCoil) => parse_write_single_coil(pdu)?,
        Ok(FunctionCode::WriteSingleRegister) => parse_write_single_register(pdu)?,
        Ok(FunctionCode::WriteMultipleCoils) => parse_write_multiple_coils(pdu)?,
        Ok(FunctionCode::WriteMultipleRegisters) => parse_write_multiple_registers(pdu)?,
        Ok(FunctionCode::MaskWriteRegister) => parse_mask_write_register(pdu)?,
        Ok(FunctionCode::ReadWriteMultipleRegisters) => parse_read_write_multiple_registers(pdu)?,
        Err(ModbusError::InvalidFunction(_)) => SemanticRequest::Custom {
            function_code,
            payload: pdu[1..].to_vec(),
        },
        Err(_) => return Err(ExceptionCode::IllegalFunction),
    };

    if is_broadcast && !request.allows_broadcast() {
        return Err(ExceptionCode::IllegalFunction);
    }

    Ok(request)
}

impl SemanticResponse {
    /// Encode the typed response back into a wire PDU.
    pub fn encode(self) -> ModbusResult<ResponsePdu> {
        let bytes = match self {
            Self::Bits { function, values } => {
                let mut response = vec![function as u8, values.len().div_ceil(8) as u8];
                response.extend(pack_bits(&values));
                response
            }
            Self::Registers { function, values } => {
                let mut response = vec![function as u8, (values.len() * 2) as u8];
                for value in values {
                    response.extend_from_slice(&value.to_be_bytes());
                }
                response
            }
            Self::WriteSingleCoilAck { address, value } => {
                let mut response = vec![FunctionCode::WriteSingleCoil as u8];
                response.extend_from_slice(&address.to_be_bytes());
                response
                    .extend_from_slice(&(if value { 0xFF00u16 } else { 0x0000u16 }).to_be_bytes());
                response
            }
            Self::WriteSingleRegisterAck { address, value } => {
                let mut response = vec![FunctionCode::WriteSingleRegister as u8];
                response.extend_from_slice(&address.to_be_bytes());
                response.extend_from_slice(&value.to_be_bytes());
                response
            }
            Self::WriteMultipleAck {
                function,
                address,
                quantity,
            } => {
                let mut response = vec![function as u8];
                response.extend_from_slice(&address.to_be_bytes());
                response.extend_from_slice(&quantity.to_be_bytes());
                response
            }
            Self::MaskWriteAck {
                address,
                and_mask,
                or_mask,
            } => {
                let mut response = vec![FunctionCode::MaskWriteRegister as u8];
                response.extend_from_slice(&address.to_be_bytes());
                response.extend_from_slice(&and_mask.to_be_bytes());
                response.extend_from_slice(&or_mask.to_be_bytes());
                response
            }
            Self::Custom(response) => return Ok(response),
        };

        ResponsePdu::new(bytes)
    }
}

fn parse_read_bits(function: FunctionCode, pdu: &[u8]) -> Result<SemanticRequest, ExceptionCode> {
    ensure_len(pdu, 5)?;
    let address = u16::from_be_bytes([pdu[1], pdu[2]]);
    let quantity = u16::from_be_bytes([pdu[3], pdu[4]]);
    ensure_quantity(quantity, 2000)?;
    ensure_span(address, quantity)?;

    Ok(match function {
        FunctionCode::ReadCoils => SemanticRequest::ReadCoils { address, quantity },
        FunctionCode::ReadDiscreteInputs => {
            SemanticRequest::ReadDiscreteInputs { address, quantity }
        }
        _ => unreachable!("read bit parser only supports FC01/FC02"),
    })
}

fn parse_read_registers(
    function: FunctionCode,
    pdu: &[u8],
) -> Result<SemanticRequest, ExceptionCode> {
    ensure_len(pdu, 5)?;
    let address = u16::from_be_bytes([pdu[1], pdu[2]]);
    let quantity = u16::from_be_bytes([pdu[3], pdu[4]]);
    ensure_quantity(quantity, 125)?;
    ensure_span(address, quantity)?;

    Ok(match function {
        FunctionCode::ReadHoldingRegisters => {
            SemanticRequest::ReadHoldingRegisters { address, quantity }
        }
        FunctionCode::ReadInputRegisters => {
            SemanticRequest::ReadInputRegisters { address, quantity }
        }
        _ => unreachable!("read register parser only supports FC03/FC04"),
    })
}

fn parse_write_single_coil(pdu: &[u8]) -> Result<SemanticRequest, ExceptionCode> {
    ensure_len(pdu, 5)?;
    let address = u16::from_be_bytes([pdu[1], pdu[2]]);
    let raw_value = u16::from_be_bytes([pdu[3], pdu[4]]);
    let value = match raw_value {
        0x0000 => false,
        0xFF00 => true,
        _ => return Err(ExceptionCode::IllegalDataValue),
    };

    Ok(SemanticRequest::WriteSingleCoil { address, value })
}

fn parse_write_single_register(pdu: &[u8]) -> Result<SemanticRequest, ExceptionCode> {
    ensure_len(pdu, 5)?;
    let address = u16::from_be_bytes([pdu[1], pdu[2]]);
    let value = u16::from_be_bytes([pdu[3], pdu[4]]);
    Ok(SemanticRequest::WriteSingleRegister { address, value })
}

fn parse_write_multiple_coils(pdu: &[u8]) -> Result<SemanticRequest, ExceptionCode> {
    ensure_len(pdu, 6)?;
    let address = u16::from_be_bytes([pdu[1], pdu[2]]);
    let quantity = u16::from_be_bytes([pdu[3], pdu[4]]);
    ensure_quantity(quantity, 1968)?;
    ensure_span(address, quantity)?;

    let byte_count = pdu[5] as usize;
    let expected = quantity.div_ceil(8) as usize;
    if byte_count != expected || pdu.len() < 6 + byte_count {
        return Err(ExceptionCode::IllegalDataValue);
    }

    let values = unpack_bits(&pdu[6..6 + byte_count], quantity as usize);
    Ok(SemanticRequest::WriteMultipleCoils { address, values })
}

fn parse_write_multiple_registers(pdu: &[u8]) -> Result<SemanticRequest, ExceptionCode> {
    ensure_len(pdu, 6)?;
    let address = u16::from_be_bytes([pdu[1], pdu[2]]);
    let quantity = u16::from_be_bytes([pdu[3], pdu[4]]);
    ensure_quantity(quantity, 123)?;
    ensure_span(address, quantity)?;

    let byte_count = pdu[5] as usize;
    let expected = quantity as usize * 2;
    if byte_count != expected || pdu.len() < 6 + byte_count {
        return Err(ExceptionCode::IllegalDataValue);
    }

    let mut values = Vec::with_capacity(quantity as usize);
    for chunk in pdu[6..6 + byte_count].chunks_exact(2) {
        values.push(u16::from_be_bytes([chunk[0], chunk[1]]));
    }

    Ok(SemanticRequest::WriteMultipleRegisters { address, values })
}

fn parse_mask_write_register(pdu: &[u8]) -> Result<SemanticRequest, ExceptionCode> {
    ensure_len(pdu, 7)?;
    let address = u16::from_be_bytes([pdu[1], pdu[2]]);
    let and_mask = u16::from_be_bytes([pdu[3], pdu[4]]);
    let or_mask = u16::from_be_bytes([pdu[5], pdu[6]]);
    Ok(SemanticRequest::MaskWriteRegister {
        address,
        and_mask,
        or_mask,
    })
}

fn parse_read_write_multiple_registers(pdu: &[u8]) -> Result<SemanticRequest, ExceptionCode> {
    ensure_len(pdu, 10)?;
    let read_address = u16::from_be_bytes([pdu[1], pdu[2]]);
    let read_quantity = u16::from_be_bytes([pdu[3], pdu[4]]);
    ensure_quantity(read_quantity, 125)?;
    ensure_span(read_address, read_quantity)?;

    let write_address = u16::from_be_bytes([pdu[5], pdu[6]]);
    let write_quantity = u16::from_be_bytes([pdu[7], pdu[8]]);
    ensure_quantity(write_quantity, 121)?;
    ensure_span(write_address, write_quantity)?;

    let byte_count = pdu[9] as usize;
    let expected = write_quantity as usize * 2;
    if byte_count != expected || pdu.len() < 10 + byte_count {
        return Err(ExceptionCode::IllegalDataValue);
    }

    let mut values = Vec::with_capacity(write_quantity as usize);
    for chunk in pdu[10..10 + byte_count].chunks_exact(2) {
        values.push(u16::from_be_bytes([chunk[0], chunk[1]]));
    }

    Ok(SemanticRequest::ReadWriteMultipleRegisters {
        read_address,
        read_quantity,
        write_address,
        values,
    })
}

fn ensure_len(pdu: &[u8], minimum: usize) -> Result<(), ExceptionCode> {
    if pdu.len() < minimum {
        Err(ExceptionCode::IllegalDataValue)
    } else {
        Ok(())
    }
}

fn ensure_quantity(quantity: u16, maximum: u16) -> Result<(), ExceptionCode> {
    if quantity == 0 || quantity > maximum {
        Err(ExceptionCode::IllegalDataValue)
    } else {
        Ok(())
    }
}

fn ensure_span(address: u16, quantity: u16) -> Result<(), ExceptionCode> {
    if quantity == 0 {
        return Err(ExceptionCode::IllegalDataValue);
    }

    let end = address as u32 + quantity as u32 - 1;
    if end > u16::MAX as u32 {
        Err(ExceptionCode::IllegalDataAddress)
    } else {
        Ok(())
    }
}

fn unpack_bits(data: &[u8], quantity: usize) -> Vec<bool> {
    let mut values = Vec::with_capacity(quantity);
    for index in 0..quantity {
        values.push((data[index / 8] & (1 << (index % 8))) != 0);
    }
    values
}

fn pack_bits(values: &[bool]) -> Vec<u8> {
    let mut packed = vec![0u8; values.len().div_ceil(8)];
    for (index, value) in values.iter().enumerate() {
        if *value {
            packed[index / 8] |= 1 << (index % 8);
        }
    }
    packed
}

#[cfg(test)]
mod tests {
    use super::{ExceptionCode, FunctionCode, RequestPdu, SemanticRequest, SemanticResponse};

    #[test]
    fn semantic_parser_enforces_broadcast_legality() {
        let request =
            RequestPdu::new([FunctionCode::ReadHoldingRegisters as u8, 0, 0, 0, 1]).unwrap();
        let error = request.semantic_request(true).unwrap_err();
        assert_eq!(error, ExceptionCode::IllegalFunction);
    }

    #[test]
    fn semantic_parser_accepts_custom_functions() {
        let request = RequestPdu::new([0x41, 0xAA, 0xBB]).unwrap();
        let semantic = request.semantic_request(false).unwrap();
        assert_eq!(
            semantic,
            SemanticRequest::Custom {
                function_code: 0x41,
                payload: vec![0xAA, 0xBB],
            }
        );
    }

    #[test]
    fn semantic_response_packs_bits() {
        let response = SemanticResponse::Bits {
            function: FunctionCode::ReadCoils,
            values: vec![true, false, true, true, false, false, false, false, true],
        }
        .encode()
        .unwrap();

        assert_eq!(response.as_bytes(), &[0x01, 0x02, 0x0D, 0x01]);
    }

    #[test]
    fn parser_rejects_invalid_register_write_lengths() {
        let request = RequestPdu::new([0x10, 0x00, 0x02, 0x00, 0x02, 0x03, 0xAA, 0xBB]).unwrap();
        let error = request.semantic_request(false).unwrap_err();
        assert_eq!(error, ExceptionCode::IllegalDataValue);
    }
}
