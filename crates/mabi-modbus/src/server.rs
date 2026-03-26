//! Modbus TCP server implementation.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use bytes::BytesMut;
use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tracing::{debug, error, info, instrument, warn};

use crate::config::ModbusServerConfig;
use crate::context::{AddressSpace, SharedAddressSpace};
use crate::device::ModbusDevice;
use crate::error::{ModbusError, ModbusResult};
use crate::register::RegisterStore;

/// Modbus function codes.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionCode {
    ReadCoils = 0x01,
    ReadDiscreteInputs = 0x02,
    ReadHoldingRegisters = 0x03,
    ReadInputRegisters = 0x04,
    WriteSingleCoil = 0x05,
    WriteSingleRegister = 0x06,
    WriteMultipleCoils = 0x0F,
    WriteMultipleRegisters = 0x10,
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
            0x17 => Ok(Self::ReadWriteMultipleRegisters),
            _ => Err(ModbusError::InvalidFunction(value)),
        }
    }
}

/// Modbus TCP server.
pub struct ModbusTcpServer {
    /// Configuration.
    config: ModbusServerConfig,

    /// Devices by unit ID.
    devices: DashMap<u8, Arc<ModbusDevice>>,

    /// Shared register store (for unit ID 0 / broadcast).
    shared_registers: SharedAddressSpace,

    /// Connection semaphore.
    connection_semaphore: Arc<Semaphore>,

    /// Shutdown flag.
    shutdown: Arc<AtomicBool>,

    /// Active connections count.
    active_connections: AtomicU64,

    /// Total requests processed.
    total_requests: AtomicU64,

    /// Start time.
    start_time: RwLock<Option<Instant>>,
}

impl ModbusTcpServer {
    /// Create a new Modbus TCP server.
    pub fn new(config: ModbusServerConfig) -> Self {
        Self {
            connection_semaphore: Arc::new(Semaphore::new(config.max_connections)),
            config,
            devices: DashMap::new(),
            shared_registers: Arc::new(RegisterStore::with_defaults()),
            shutdown: Arc::new(AtomicBool::new(false)),
            active_connections: AtomicU64::new(0),
            total_requests: AtomicU64::new(0),
            start_time: RwLock::new(None),
        }
    }

    /// Add a device to the server.
    pub fn add_device(&self, device: ModbusDevice) {
        let unit_id = device.unit_id();
        self.devices.insert(unit_id, Arc::new(device));
    }

    /// Get device by unit ID.
    pub fn device(&self, unit_id: u8) -> Option<Arc<ModbusDevice>> {
        self.devices.get(&unit_id).map(|d| d.clone())
    }

    /// Get shared register store.
    pub fn shared_registers(&self) -> SharedAddressSpace {
        self.shared_registers.clone()
    }

    /// Get active connections count.
    pub fn active_connections(&self) -> u64 {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// Get total requests processed.
    pub fn total_requests(&self) -> u64 {
        self.total_requests.load(Ordering::Relaxed)
    }

    /// Check if shutdown is requested.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }

    /// Request shutdown.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// Run the server.
    #[instrument(skip(self))]
    pub async fn run(&self) -> ModbusResult<()> {
        let listener = TcpListener::bind(self.config.bind_address).await?;
        info!(address = %self.config.bind_address, "Modbus TCP server started");

        *self.start_time.write() = Some(Instant::now());

        while !self.is_shutdown() {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            if let Ok(permit) = self.connection_semaphore.clone().try_acquire_owned() {
                                self.active_connections.fetch_add(1, Ordering::Relaxed);

                                let devices = self.devices.clone();
                                let shared_registers = self.shared_registers.clone();
                                let shutdown = self.shutdown.clone();
                                let timeout = self.config.timeout();

                                tokio::spawn(async move {
                                    if let Err(e) = handle_connection(
                                        stream,
                                        addr,
                                        devices,
                                        shared_registers,
                                        shutdown,
                                        timeout,
                                    ).await {
                                        debug!(error = %e, "Connection error");
                                    }
                                    drop(permit);
                                });
                            } else {
                                warn!("Max connections reached, rejecting connection from {}", addr);
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "Accept error");
                        }
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                    // Check shutdown flag periodically
                }
            }
        }

        info!("Modbus TCP server stopped");
        Ok(())
    }
}

/// Handle a single connection.
async fn handle_connection(
    mut stream: TcpStream,
    addr: SocketAddr,
    devices: DashMap<u8, Arc<ModbusDevice>>,
    shared_registers: SharedAddressSpace,
    shutdown: Arc<AtomicBool>,
    timeout: std::time::Duration,
) -> ModbusResult<()> {
    debug!(addr = %addr, "New connection");

    let mut buffer = BytesMut::with_capacity(256);

    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        // Read MBAP header (7 bytes) + PDU
        buffer.clear();
        buffer.resize(256, 0);

        let read_result = tokio::time::timeout(timeout, stream.read(&mut buffer)).await;

        match read_result {
            Ok(Ok(0)) => {
                debug!(addr = %addr, "Connection closed");
                break;
            }
            Ok(Ok(n)) => {
                buffer.truncate(n);

                if n < 8 {
                    warn!(addr = %addr, "Packet too short: {} bytes", n);
                    continue;
                }

                // Parse MBAP header
                let transaction_id = u16::from_be_bytes([buffer[0], buffer[1]]);
                let protocol_id = u16::from_be_bytes([buffer[2], buffer[3]]);
                let _length = u16::from_be_bytes([buffer[4], buffer[5]]);
                let unit_id = buffer[6];

                if protocol_id != 0 {
                    warn!(addr = %addr, "Invalid protocol ID: {}", protocol_id);
                    continue;
                }

                // Get registers for this unit ID
                let registers = if let Some(device) = devices.get(&unit_id) {
                    device.address_space()
                } else if unit_id == 0 {
                    // Broadcast / shared
                    shared_registers.clone()
                } else {
                    // Send exception response
                    let response = build_exception_response(
                        transaction_id,
                        unit_id,
                        buffer[7],
                        0x0B, // Gateway Target Device Failed to Respond
                    );
                    stream.write_all(&response).await?;
                    continue;
                };

                // Process request
                let pdu = &buffer[7..];
                let response = process_request(transaction_id, unit_id, pdu, registers.as_ref())?;

                stream.write_all(&response).await?;
            }
            Ok(Err(e)) => {
                debug!(addr = %addr, error = %e, "Read error");
                break;
            }
            Err(_) => {
                debug!(addr = %addr, "Connection timeout");
                break;
            }
        }
    }

    Ok(())
}

/// Process a Modbus request.
fn process_request(
    transaction_id: u16,
    unit_id: u8,
    pdu: &[u8],
    registers: &dyn AddressSpace,
) -> ModbusResult<Vec<u8>> {
    if pdu.is_empty() {
        return Err(ModbusError::InvalidData("Empty PDU".into()));
    }

    let function_code = FunctionCode::try_from(pdu[0])?;

    let response_pdu = match function_code {
        FunctionCode::ReadCoils => {
            let address = u16::from_be_bytes([pdu[1], pdu[2]]);
            let quantity = u16::from_be_bytes([pdu[3], pdu[4]]);
            read_coils(registers, address, quantity)?
        }
        FunctionCode::ReadDiscreteInputs => {
            let address = u16::from_be_bytes([pdu[1], pdu[2]]);
            let quantity = u16::from_be_bytes([pdu[3], pdu[4]]);
            read_discrete_inputs(registers, address, quantity)?
        }
        FunctionCode::ReadHoldingRegisters => {
            let address = u16::from_be_bytes([pdu[1], pdu[2]]);
            let quantity = u16::from_be_bytes([pdu[3], pdu[4]]);
            read_holding_registers(registers, address, quantity)?
        }
        FunctionCode::ReadInputRegisters => {
            let address = u16::from_be_bytes([pdu[1], pdu[2]]);
            let quantity = u16::from_be_bytes([pdu[3], pdu[4]]);
            read_input_registers(registers, address, quantity)?
        }
        FunctionCode::WriteSingleCoil => {
            let address = u16::from_be_bytes([pdu[1], pdu[2]]);
            let value = u16::from_be_bytes([pdu[3], pdu[4]]);
            write_single_coil(registers, address, value)?
        }
        FunctionCode::WriteSingleRegister => {
            let address = u16::from_be_bytes([pdu[1], pdu[2]]);
            let value = u16::from_be_bytes([pdu[3], pdu[4]]);
            write_single_register(registers, address, value)?
        }
        FunctionCode::WriteMultipleCoils => {
            let address = u16::from_be_bytes([pdu[1], pdu[2]]);
            let quantity = u16::from_be_bytes([pdu[3], pdu[4]]);
            let byte_count = pdu[5] as usize;
            let data = &pdu[6..6 + byte_count];
            write_multiple_coils(registers, address, quantity, data)?
        }
        FunctionCode::WriteMultipleRegisters => {
            let address = u16::from_be_bytes([pdu[1], pdu[2]]);
            let quantity = u16::from_be_bytes([pdu[3], pdu[4]]);
            let byte_count = pdu[5] as usize;
            let data = &pdu[6..6 + byte_count];
            write_multiple_registers(registers, address, quantity, data)?
        }
        FunctionCode::ReadWriteMultipleRegisters => {
            let read_address = u16::from_be_bytes([pdu[1], pdu[2]]);
            let read_quantity = u16::from_be_bytes([pdu[3], pdu[4]]);
            let write_address = u16::from_be_bytes([pdu[5], pdu[6]]);
            let write_quantity = u16::from_be_bytes([pdu[7], pdu[8]]);
            let byte_count = pdu[9] as usize;
            let data = &pdu[10..10 + byte_count];
            read_write_multiple_registers(
                registers,
                read_address,
                read_quantity,
                write_address,
                write_quantity,
                data,
            )?
        }
    };

    // Build MBAP + PDU response
    Ok(build_response(transaction_id, unit_id, &response_pdu))
}

/// Build MBAP + PDU response.
fn build_response(transaction_id: u16, unit_id: u8, pdu: &[u8]) -> Vec<u8> {
    let length = (pdu.len() + 1) as u16; // PDU + unit_id

    let mut response = Vec::with_capacity(7 + pdu.len());
    response.extend_from_slice(&transaction_id.to_be_bytes());
    response.extend_from_slice(&0u16.to_be_bytes()); // Protocol ID
    response.extend_from_slice(&length.to_be_bytes());
    response.push(unit_id);
    response.extend_from_slice(pdu);

    response
}

/// Build exception response.
fn build_exception_response(
    transaction_id: u16,
    unit_id: u8,
    function_code: u8,
    exception_code: u8,
) -> Vec<u8> {
    let pdu = vec![function_code | 0x80, exception_code];
    build_response(transaction_id, unit_id, &pdu)
}

// Function handlers

fn read_coils(registers: &dyn AddressSpace, address: u16, quantity: u16) -> ModbusResult<Vec<u8>> {
    let coils = registers.read_coils(address, quantity)?;
    let byte_count = quantity.div_ceil(8);

    let mut response = vec![0x01, byte_count as u8];
    let mut bytes = vec![0u8; byte_count as usize];

    for (i, &coil) in coils.iter().enumerate() {
        if coil {
            bytes[i / 8] |= 1 << (i % 8);
        }
    }

    response.extend_from_slice(&bytes);
    Ok(response)
}

fn read_discrete_inputs(
    registers: &dyn AddressSpace,
    address: u16,
    quantity: u16,
) -> ModbusResult<Vec<u8>> {
    let inputs = registers.read_discrete_inputs(address, quantity)?;
    let byte_count = quantity.div_ceil(8);

    let mut response = vec![0x02, byte_count as u8];
    let mut bytes = vec![0u8; byte_count as usize];

    for (i, &input) in inputs.iter().enumerate() {
        if input {
            bytes[i / 8] |= 1 << (i % 8);
        }
    }

    response.extend_from_slice(&bytes);
    Ok(response)
}

fn read_holding_registers(
    registers: &dyn AddressSpace,
    address: u16,
    quantity: u16,
) -> ModbusResult<Vec<u8>> {
    let values = registers.read_holding_registers(address, quantity)?;
    let byte_count = (quantity * 2) as u8;

    let mut response = vec![0x03, byte_count];
    for value in values {
        response.extend_from_slice(&value.to_be_bytes());
    }

    Ok(response)
}

fn read_input_registers(
    registers: &dyn AddressSpace,
    address: u16,
    quantity: u16,
) -> ModbusResult<Vec<u8>> {
    let values = registers.read_input_registers(address, quantity)?;
    let byte_count = (quantity * 2) as u8;

    let mut response = vec![0x04, byte_count];
    for value in values {
        response.extend_from_slice(&value.to_be_bytes());
    }

    Ok(response)
}

fn write_single_coil(
    registers: &dyn AddressSpace,
    address: u16,
    value: u16,
) -> ModbusResult<Vec<u8>> {
    let coil_value = value == 0xFF00;
    registers.write_coil(address, coil_value)?;

    let mut response = vec![0x05];
    response.extend_from_slice(&address.to_be_bytes());
    response.extend_from_slice(&value.to_be_bytes());

    Ok(response)
}

fn write_single_register(
    registers: &dyn AddressSpace,
    address: u16,
    value: u16,
) -> ModbusResult<Vec<u8>> {
    registers.write_holding_register(address, value)?;

    let mut response = vec![0x06];
    response.extend_from_slice(&address.to_be_bytes());
    response.extend_from_slice(&value.to_be_bytes());

    Ok(response)
}

fn write_multiple_coils(
    registers: &dyn AddressSpace,
    address: u16,
    quantity: u16,
    data: &[u8],
) -> ModbusResult<Vec<u8>> {
    let mut coils = Vec::with_capacity(quantity as usize);
    for i in 0..quantity as usize {
        coils.push((data[i / 8] & (1 << (i % 8))) != 0);
    }

    registers.write_coils(address, &coils)?;

    let mut response = vec![0x0F];
    response.extend_from_slice(&address.to_be_bytes());
    response.extend_from_slice(&quantity.to_be_bytes());

    Ok(response)
}

fn write_multiple_registers(
    registers: &dyn AddressSpace,
    address: u16,
    quantity: u16,
    data: &[u8],
) -> ModbusResult<Vec<u8>> {
    let mut values = Vec::with_capacity(quantity as usize);
    for i in 0..quantity as usize {
        values.push(u16::from_be_bytes([data[i * 2], data[i * 2 + 1]]));
    }

    registers.write_holding_registers(address, &values)?;

    let mut response = vec![0x10];
    response.extend_from_slice(&address.to_be_bytes());
    response.extend_from_slice(&quantity.to_be_bytes());

    Ok(response)
}

fn read_write_multiple_registers(
    registers: &dyn AddressSpace,
    read_address: u16,
    read_quantity: u16,
    write_address: u16,
    write_quantity: u16,
    data: &[u8],
) -> ModbusResult<Vec<u8>> {
    // Write first
    let mut write_values = Vec::with_capacity(write_quantity as usize);
    for i in 0..write_quantity as usize {
        write_values.push(u16::from_be_bytes([data[i * 2], data[i * 2 + 1]]));
    }
    registers.write_holding_registers(write_address, &write_values)?;

    // Then read
    let read_values = registers.read_holding_registers(read_address, read_quantity)?;
    let byte_count = (read_quantity * 2) as u8;

    let mut response = vec![0x17, byte_count];
    for value in read_values {
        response.extend_from_slice(&value.to_be_bytes());
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_code_conversion() {
        assert_eq!(
            FunctionCode::try_from(0x01).unwrap(),
            FunctionCode::ReadCoils
        );
        assert_eq!(
            FunctionCode::try_from(0x03).unwrap(),
            FunctionCode::ReadHoldingRegisters
        );
        assert!(FunctionCode::try_from(0xFF).is_err());
    }

    #[test]
    fn test_process_read_holding_registers() {
        let registers = RegisterStore::with_defaults();
        registers
            .write_holding_registers(0, &[100, 200, 300])
            .unwrap();

        let pdu = [0x03, 0x00, 0x00, 0x00, 0x03]; // Read 3 registers from address 0
        let response = process_request(1, 1, &pdu, &registers).unwrap();

        // MBAP (7) + Function (1) + Byte count (1) + Data (6) = 15 bytes
        assert_eq!(response.len(), 15);
        assert_eq!(response[7], 0x03); // Function code
        assert_eq!(response[8], 6); // Byte count
    }

    #[test]
    fn test_process_write_single_register() {
        let registers = RegisterStore::with_defaults();

        let pdu = [0x06, 0x00, 0x0A, 0x12, 0x34]; // Write 0x1234 to address 10
        let response = process_request(1, 1, &pdu, &registers).unwrap();

        // Verify response echoes the request
        assert_eq!(response[7], 0x06);

        // Verify the register was written
        let values = registers.read_holding_registers(10, 1).unwrap();
        assert_eq!(values[0], 0x1234);
    }
}
