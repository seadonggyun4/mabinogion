//! Canonical built-in Modbus semantic execution core.

use crate::context::{AddressSpace, RequestTarget};
use crate::core::{ExceptionCode, FunctionCode, SemanticRequest, SemanticResponse};
use crate::error::ModbusError;

/// Typed failure surface for the canonical semantic core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticFailure {
    IllegalFunction,
    IllegalDataAddress,
    IllegalDataValue,
    SlaveDeviceFailure,
}

impl SemanticFailure {
    /// Maps the failure into the corresponding Modbus exception code.
    pub fn exception_code(self) -> ExceptionCode {
        match self {
            Self::IllegalFunction => ExceptionCode::IllegalFunction,
            Self::IllegalDataAddress => ExceptionCode::IllegalDataAddress,
            Self::IllegalDataValue => ExceptionCode::IllegalDataValue,
            Self::SlaveDeviceFailure => ExceptionCode::SlaveDeviceFailure,
        }
    }

    /// Maps a datastore/runtime error into a canonical semantic failure.
    pub fn from_modbus_error(error: ModbusError) -> Self {
        match error {
            ModbusError::InvalidAddress { .. } | ModbusError::DeviceNotFound { .. } => {
                Self::IllegalDataAddress
            }
            ModbusError::InvalidQuantity { .. } => Self::IllegalDataAddress,
            ModbusError::InvalidData(_) => Self::IllegalDataValue,
            _ => Self::SlaveDeviceFailure,
        }
    }
}

impl From<ExceptionCode> for SemanticFailure {
    fn from(value: ExceptionCode) -> Self {
        match value {
            ExceptionCode::IllegalFunction => Self::IllegalFunction,
            ExceptionCode::IllegalDataAddress => Self::IllegalDataAddress,
            ExceptionCode::IllegalDataValue => Self::IllegalDataValue,
            _ => Self::SlaveDeviceFailure,
        }
    }
}

/// Canonical execution engine for built-in Modbus function codes.
#[derive(Debug, Clone, Copy, Default)]
pub struct SemanticCore;

impl SemanticCore {
    /// Executes a unicast request against a single target.
    pub fn execute_unicast(
        self,
        request: &SemanticRequest,
        target: &RequestTarget,
    ) -> Result<SemanticResponse, SemanticFailure> {
        execute_builtin(request, target.address_space().as_ref())
    }

    /// Executes a broadcast-safe request across every selected target.
    pub fn execute_broadcast(
        self,
        request: &SemanticRequest,
        targets: &[RequestTarget],
    ) -> Result<SemanticResponse, SemanticFailure> {
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
            _ => return Err(SemanticFailure::IllegalFunction),
        })
    }
}

fn execute_builtin(
    request: &SemanticRequest,
    address_space: &dyn AddressSpace,
) -> Result<SemanticResponse, SemanticFailure> {
    match request {
        SemanticRequest::ReadCoils { address, quantity } => address_space
            .read_coils(*address, *quantity)
            .map(|values| SemanticResponse::Bits {
                function: FunctionCode::ReadCoils,
                values,
            })
            .map_err(SemanticFailure::from_modbus_error),
        SemanticRequest::ReadDiscreteInputs { address, quantity } => address_space
            .read_discrete_inputs(*address, *quantity)
            .map(|values| SemanticResponse::Bits {
                function: FunctionCode::ReadDiscreteInputs,
                values,
            })
            .map_err(SemanticFailure::from_modbus_error),
        SemanticRequest::ReadHoldingRegisters { address, quantity } => address_space
            .read_holding_registers(*address, *quantity)
            .map(|values| SemanticResponse::Registers {
                function: FunctionCode::ReadHoldingRegisters,
                values,
            })
            .map_err(SemanticFailure::from_modbus_error),
        SemanticRequest::ReadInputRegisters { address, quantity } => address_space
            .read_input_registers(*address, *quantity)
            .map(|values| SemanticResponse::Registers {
                function: FunctionCode::ReadInputRegisters,
                values,
            })
            .map_err(SemanticFailure::from_modbus_error),
        SemanticRequest::WriteSingleCoil { address, value } => {
            address_space
                .write_coil(*address, *value)
                .map_err(SemanticFailure::from_modbus_error)?;
            Ok(SemanticResponse::WriteSingleCoilAck {
                address: *address,
                value: *value,
            })
        }
        SemanticRequest::WriteSingleRegister { address, value } => {
            address_space
                .write_holding_register(*address, *value)
                .map_err(SemanticFailure::from_modbus_error)?;
            Ok(SemanticResponse::WriteSingleRegisterAck {
                address: *address,
                value: *value,
            })
        }
        SemanticRequest::WriteMultipleCoils { address, values } => {
            address_space
                .write_coils(*address, values)
                .map_err(SemanticFailure::from_modbus_error)?;
            Ok(SemanticResponse::WriteMultipleAck {
                function: FunctionCode::WriteMultipleCoils,
                address: *address,
                quantity: values.len() as u16,
            })
        }
        SemanticRequest::WriteMultipleRegisters { address, values } => {
            address_space
                .write_holding_registers(*address, values)
                .map_err(SemanticFailure::from_modbus_error)?;
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
                .map_err(SemanticFailure::from_modbus_error)?;
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
                .map_err(SemanticFailure::from_modbus_error)?;
            let registers = address_space
                .read_holding_registers(*read_address, *read_quantity)
                .map_err(SemanticFailure::from_modbus_error)?;
            Ok(SemanticResponse::Registers {
                function: FunctionCode::ReadWriteMultipleRegisters,
                values: registers,
            })
        }
        SemanticRequest::Custom { .. } => Err(SemanticFailure::IllegalFunction),
    }
}

fn execute_builtin_write_only(
    request: &SemanticRequest,
    address_space: &dyn AddressSpace,
) -> Result<(), SemanticFailure> {
    match request {
        SemanticRequest::WriteSingleCoil { address, value } => address_space
            .write_coil(*address, *value)
            .map_err(SemanticFailure::from_modbus_error),
        SemanticRequest::WriteSingleRegister { address, value } => address_space
            .write_holding_register(*address, *value)
            .map_err(SemanticFailure::from_modbus_error),
        SemanticRequest::WriteMultipleCoils { address, values } => address_space
            .write_coils(*address, values)
            .map_err(SemanticFailure::from_modbus_error),
        SemanticRequest::WriteMultipleRegisters { address, values } => address_space
            .write_holding_registers(*address, values)
            .map_err(SemanticFailure::from_modbus_error),
        SemanticRequest::MaskWriteRegister {
            address,
            and_mask,
            or_mask,
        } => address_space
            .mask_write_holding_register(*address, *and_mask, *or_mask)
            .map_err(SemanticFailure::from_modbus_error),
        _ => Err(SemanticFailure::IllegalFunction),
    }
}
