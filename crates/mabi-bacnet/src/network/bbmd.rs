//! BBMD (BACnet Broadcast Management Device) implementation.
//!
//! BBMD enables BACnet/IP devices to communicate across subnet boundaries
//! by managing broadcast distribution and foreign device registration.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                         BBMD                                 │
//! │                                                             │
//! │  ┌─────────────────────┐  ┌─────────────────────────────┐  │
//! │  │ BroadcastDistTable  │  │   ForeignDeviceTable        │  │
//! │  │ (BDT Entries)       │  │   (FDT Entries)             │  │
//! │  └──────────┬──────────┘  └────────────┬────────────────┘  │
//! │             │                          │                   │
//! │             ▼                          ▼                   │
//! │  ┌──────────────────────────────────────────────────────┐  │
//! │  │              Message Forwarding                       │  │
//! │  │   (Original → Forwarded-NPDU distribution)           │  │
//! │  └──────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Features
//!
//! - **Broadcast Distribution Table (BDT)**: Routes broadcasts to peer BBMDs
//! - **Foreign Device Table (FDT)**: Manages registered foreign devices
//! - **Automatic Expiration**: Removes stale foreign device registrations
//! - **Thread-Safe**: Uses DashMap for concurrent access

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use tracing::{debug, info, warn};

use super::bvlc::{BvlcFunction, BvlcMessage, BvlcResultCode};

// ============================================================================
// Broadcast Distribution Table (BDT)
// ============================================================================

/// Entry in the Broadcast Distribution Table.
#[derive(Debug, Clone)]
pub struct BdtEntry {
    /// IP address of the peer BBMD.
    pub address: SocketAddrV4,
    /// Broadcast distribution mask (for directed broadcasts).
    pub broadcast_mask: Ipv4Addr,
    /// Time this entry was added.
    pub added_at: Instant,
}

impl BdtEntry {
    /// Create a new BDT entry.
    pub fn new(address: SocketAddrV4, broadcast_mask: Ipv4Addr) -> Self {
        Self {
            address,
            broadcast_mask,
            added_at: Instant::now(),
        }
    }

    /// Get the directed broadcast address for this entry.
    pub fn directed_broadcast_address(&self) -> Ipv4Addr {
        let ip = self.address.ip();
        let mask = self.broadcast_mask;
        Ipv4Addr::new(
            ip.octets()[0] | !mask.octets()[0],
            ip.octets()[1] | !mask.octets()[1],
            ip.octets()[2] | !mask.octets()[2],
            ip.octets()[3] | !mask.octets()[3],
        )
    }
}

/// Broadcast Distribution Table.
///
/// Stores information about peer BBMDs for cross-subnet broadcast forwarding.
#[derive(Debug)]
pub struct BroadcastDistributionTable {
    entries: DashMap<SocketAddrV4, BdtEntry>,
    max_entries: usize,
}

impl BroadcastDistributionTable {
    /// Create a new BDT with the specified maximum entries.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: DashMap::new(),
            max_entries,
        }
    }

    /// Add or update an entry.
    pub fn add(&self, entry: BdtEntry) -> Result<(), BbmdError> {
        if self.entries.len() >= self.max_entries && !self.entries.contains_key(&entry.address) {
            return Err(BbmdError::TableFull);
        }
        self.entries.insert(entry.address, entry);
        Ok(())
    }

    /// Remove an entry.
    pub fn remove(&self, address: &SocketAddrV4) -> Option<BdtEntry> {
        self.entries.remove(address).map(|(_, v)| v)
    }

    /// Get all entries.
    pub fn entries(&self) -> Vec<BdtEntry> {
        self.entries.iter().map(|r| r.value().clone()).collect()
    }

    /// Get the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all entries.
    pub fn clear(&self) {
        self.entries.clear();
    }

    /// Encode the BDT for transmission.
    pub fn encode(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(self.entries.len() * 10);
        for entry in self.entries.iter() {
            // IP address (4 bytes) + port (2 bytes) + mask (4 bytes) = 10 bytes
            data.extend_from_slice(&entry.address.ip().octets());
            data.extend_from_slice(&entry.address.port().to_be_bytes());
            data.extend_from_slice(&entry.broadcast_mask.octets());
        }
        data
    }

    /// Decode and replace the BDT from received data.
    pub fn decode_and_replace(&self, data: &[u8]) -> Result<(), BbmdError> {
        if data.len() % 10 != 0 {
            return Err(BbmdError::InvalidFormat);
        }

        self.entries.clear();

        for chunk in data.chunks(10) {
            let ip = Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]);
            let port = u16::from_be_bytes([chunk[4], chunk[5]]);
            let mask = Ipv4Addr::new(chunk[6], chunk[7], chunk[8], chunk[9]);

            let entry = BdtEntry::new(SocketAddrV4::new(ip, port), mask);
            self.entries.insert(entry.address, entry);
        }

        Ok(())
    }
}

impl Default for BroadcastDistributionTable {
    fn default() -> Self {
        Self::new(128) // Default max entries
    }
}

// ============================================================================
// Foreign Device Table (FDT)
// ============================================================================

/// Entry in the Foreign Device Table.
#[derive(Debug, Clone)]
pub struct FdtEntry {
    /// IP address of the foreign device.
    pub address: SocketAddrV4,
    /// Time-to-live in seconds (as registered).
    pub ttl_seconds: u16,
    /// Time remaining until expiration.
    pub time_remaining: Duration,
    /// Time this entry was last updated.
    pub registered_at: Instant,
}

impl FdtEntry {
    /// Create a new FDT entry.
    pub fn new(address: SocketAddrV4, ttl_seconds: u16) -> Self {
        Self {
            address,
            ttl_seconds,
            time_remaining: Duration::from_secs(ttl_seconds as u64),
            registered_at: Instant::now(),
        }
    }

    /// Check if this entry has expired.
    pub fn is_expired(&self) -> bool {
        self.registered_at.elapsed() > Duration::from_secs(self.ttl_seconds as u64)
    }

    /// Get the remaining time until expiration.
    pub fn remaining_time(&self) -> Duration {
        let elapsed = self.registered_at.elapsed();
        let ttl = Duration::from_secs(self.ttl_seconds as u64);
        ttl.saturating_sub(elapsed)
    }

    /// Refresh the registration.
    pub fn refresh(&mut self, ttl_seconds: u16) {
        self.ttl_seconds = ttl_seconds;
        self.time_remaining = Duration::from_secs(ttl_seconds as u64);
        self.registered_at = Instant::now();
    }
}

/// Foreign Device Table.
///
/// Stores information about foreign devices that have registered with this BBMD.
#[derive(Debug)]
pub struct ForeignDeviceTable {
    entries: DashMap<SocketAddrV4, FdtEntry>,
    max_entries: usize,
}

impl ForeignDeviceTable {
    /// Create a new FDT with the specified maximum entries.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: DashMap::new(),
            max_entries,
        }
    }

    /// Register or refresh a foreign device.
    pub fn register(&self, address: SocketAddrV4, ttl_seconds: u16) -> Result<(), BbmdError> {
        if let Some(mut entry) = self.entries.get_mut(&address) {
            entry.refresh(ttl_seconds);
            debug!(
                ?address,
                ttl_seconds, "Foreign device registration refreshed"
            );
            return Ok(());
        }

        if self.entries.len() >= self.max_entries {
            return Err(BbmdError::TableFull);
        }

        let entry = FdtEntry::new(address, ttl_seconds);
        self.entries.insert(address, entry);
        info!(?address, ttl_seconds, "Foreign device registered");
        Ok(())
    }

    /// Remove a foreign device.
    pub fn remove(&self, address: &SocketAddrV4) -> Option<FdtEntry> {
        self.entries.remove(address).map(|(_, v)| v)
    }

    /// Get all entries (including expired).
    pub fn entries(&self) -> Vec<FdtEntry> {
        self.entries.iter().map(|r| r.value().clone()).collect()
    }

    /// Get all active (non-expired) entries.
    pub fn active_entries(&self) -> Vec<FdtEntry> {
        self.entries
            .iter()
            .filter(|r| !r.value().is_expired())
            .map(|r| r.value().clone())
            .collect()
    }

    /// Get the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove expired entries.
    pub fn cleanup_expired(&self) -> usize {
        let expired: Vec<SocketAddrV4> = self
            .entries
            .iter()
            .filter(|r| r.value().is_expired())
            .map(|r| *r.key())
            .collect();

        let count = expired.len();
        for addr in expired {
            if let Some((_, entry)) = self.entries.remove(&addr) {
                debug!(?addr, "Foreign device registration expired");
                drop(entry);
            }
        }

        if count > 0 {
            info!(count, "Cleaned up expired foreign device registrations");
        }

        count
    }

    /// Check if an address is a registered foreign device.
    pub fn is_registered(&self, address: &SocketAddrV4) -> bool {
        self.entries
            .get(address)
            .map(|e| !e.is_expired())
            .unwrap_or(false)
    }

    /// Encode the FDT for transmission.
    pub fn encode(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(self.entries.len() * 10);
        for entry in self.entries.iter() {
            if entry.is_expired() {
                continue;
            }
            // IP address (4 bytes) + port (2 bytes) + TTL (2 bytes) + remaining (2 bytes) = 10 bytes
            data.extend_from_slice(&entry.address.ip().octets());
            data.extend_from_slice(&entry.address.port().to_be_bytes());
            data.extend_from_slice(&entry.ttl_seconds.to_be_bytes());
            let remaining_secs = entry.remaining_time().as_secs().min(u16::MAX as u64) as u16;
            data.extend_from_slice(&remaining_secs.to_be_bytes());
        }
        data
    }
}

impl Default for ForeignDeviceTable {
    fn default() -> Self {
        Self::new(256) // Default max entries
    }
}

// ============================================================================
// BBMD Manager
// ============================================================================

/// Configuration for the BBMD.
#[derive(Debug, Clone)]
pub struct BbmdConfig {
    /// Enable BBMD functionality.
    pub enabled: bool,
    /// Maximum BDT entries.
    pub max_bdt_entries: usize,
    /// Maximum FDT entries.
    pub max_fdt_entries: usize,
    /// Interval for FDT cleanup.
    pub cleanup_interval: Duration,
    /// Accept foreign device registrations.
    pub accept_foreign_devices: bool,
}

impl Default for BbmdConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_bdt_entries: 128,
            max_fdt_entries: 256,
            cleanup_interval: Duration::from_secs(30),
            accept_foreign_devices: true,
        }
    }
}

impl BbmdConfig {
    /// Create a new config with BBMD enabled.
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    /// Set whether to accept foreign device registrations.
    pub fn with_accept_foreign_devices(mut self, accept: bool) -> Self {
        self.accept_foreign_devices = accept;
        self
    }
}

/// BBMD (BACnet Broadcast Management Device) manager.
///
/// Handles broadcast distribution and foreign device registration
/// for cross-subnet BACnet/IP communication.
pub struct Bbmd {
    config: BbmdConfig,
    bdt: BroadcastDistributionTable,
    fdt: ForeignDeviceTable,
}

impl Bbmd {
    /// Create a new BBMD with the specified configuration.
    pub fn new(config: BbmdConfig) -> Self {
        Self {
            bdt: BroadcastDistributionTable::new(config.max_bdt_entries),
            fdt: ForeignDeviceTable::new(config.max_fdt_entries),
            config,
        }
    }

    /// Check if BBMD is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the BDT.
    pub fn bdt(&self) -> &BroadcastDistributionTable {
        &self.bdt
    }

    /// Get the FDT.
    pub fn fdt(&self) -> &ForeignDeviceTable {
        &self.fdt
    }

    /// Handle a BVLC message and return a response if needed.
    pub fn handle_message(&self, msg: &BvlcMessage, source: SocketAddrV4) -> Option<BvlcMessage> {
        if !self.config.enabled {
            return None;
        }

        match msg.header.function {
            BvlcFunction::RegisterForeignDevice => {
                self.handle_register_foreign_device(source, &msg.npdu)
            }
            BvlcFunction::ReadForeignDeviceTable => self.handle_read_fdt(),
            BvlcFunction::DeleteForeignDeviceTableEntry => {
                self.handle_delete_fdt_entry(source, &msg.npdu)
            }
            BvlcFunction::ReadBroadcastDistributionTable => self.handle_read_bdt(),
            BvlcFunction::WriteBroadcastDistributionTable => self.handle_write_bdt(&msg.npdu),
            BvlcFunction::DistributeBroadcastToNetwork => {
                // This is handled by the forwarding logic, not here
                None
            }
            _ => None,
        }
    }

    /// Handle Register-Foreign-Device request.
    fn handle_register_foreign_device(
        &self,
        source: SocketAddrV4,
        data: &[u8],
    ) -> Option<BvlcMessage> {
        if !self.config.accept_foreign_devices {
            warn!(?source, "Foreign device registration rejected (disabled)");
            return Some(BvlcMessage::result(
                BvlcResultCode::RegisterForeignDeviceNak,
            ));
        }

        if data.len() < 2 {
            return Some(BvlcMessage::result(
                BvlcResultCode::RegisterForeignDeviceNak,
            ));
        }

        let ttl_seconds = u16::from_be_bytes([data[0], data[1]]);

        match self.fdt.register(source, ttl_seconds) {
            Ok(()) => Some(BvlcMessage::result(BvlcResultCode::Success)),
            Err(e) => {
                warn!(?source, ?e, "Failed to register foreign device");
                Some(BvlcMessage::result(
                    BvlcResultCode::RegisterForeignDeviceNak,
                ))
            }
        }
    }

    /// Handle Read-Foreign-Device-Table request.
    fn handle_read_fdt(&self) -> Option<BvlcMessage> {
        let data = self.fdt.encode();
        let length = (4 + data.len()) as u16;

        Some(BvlcMessage {
            header: super::bvlc::BvlcHeader::new(BvlcFunction::ReadForeignDeviceTableAck, length),
            npdu: data,
            original_source: None,
            result_code: None,
        })
    }

    /// Handle Delete-Foreign-Device-Table-Entry request.
    fn handle_delete_fdt_entry(&self, _source: SocketAddrV4, data: &[u8]) -> Option<BvlcMessage> {
        if data.len() < 6 {
            return Some(BvlcMessage::result(BvlcResultCode::DeleteFdtEntryNak));
        }

        let ip = Ipv4Addr::new(data[0], data[1], data[2], data[3]);
        let port = u16::from_be_bytes([data[4], data[5]]);
        let address = SocketAddrV4::new(ip, port);

        match self.fdt.remove(&address) {
            Some(_) => {
                info!(?address, "Foreign device entry deleted");
                Some(BvlcMessage::result(BvlcResultCode::Success))
            }
            None => Some(BvlcMessage::result(BvlcResultCode::DeleteFdtEntryNak)),
        }
    }

    /// Handle Read-Broadcast-Distribution-Table request.
    fn handle_read_bdt(&self) -> Option<BvlcMessage> {
        let data = self.bdt.encode();
        let length = (4 + data.len()) as u16;

        Some(BvlcMessage {
            header: super::bvlc::BvlcHeader::new(
                BvlcFunction::ReadBroadcastDistributionTableAck,
                length,
            ),
            npdu: data,
            original_source: None,
            result_code: None,
        })
    }

    /// Handle Write-Broadcast-Distribution-Table request.
    fn handle_write_bdt(&self, data: &[u8]) -> Option<BvlcMessage> {
        match self.bdt.decode_and_replace(data) {
            Ok(()) => {
                info!(entries = self.bdt.len(), "BDT updated");
                Some(BvlcMessage::result(BvlcResultCode::Success))
            }
            Err(e) => {
                warn!(?e, "Failed to update BDT");
                Some(BvlcMessage::result(BvlcResultCode::WriteBdtNak))
            }
        }
    }

    /// Get the list of addresses to forward a broadcast to.
    ///
    /// This includes:
    /// - All BDT entries (peer BBMDs)
    /// - All registered foreign devices (non-expired)
    pub fn get_forward_addresses(&self, exclude: Option<&SocketAddrV4>) -> Vec<SocketAddrV4> {
        let mut addresses = Vec::new();

        // Add BDT entries
        for entry in self.bdt.entries() {
            if exclude.map_or(true, |e| e != &entry.address) {
                addresses.push(entry.address);
            }
        }

        // Add active foreign devices
        for entry in self.fdt.active_entries() {
            if exclude.map_or(true, |e| e != &entry.address) {
                addresses.push(entry.address);
            }
        }

        addresses
    }

    /// Perform periodic cleanup of expired entries.
    pub fn cleanup(&self) -> usize {
        self.fdt.cleanup_expired()
    }
}

impl Default for Bbmd {
    fn default() -> Self {
        Self::new(BbmdConfig::default())
    }
}

// ============================================================================
// Errors
// ============================================================================

/// BBMD errors.
#[derive(Debug, thiserror::Error)]
pub enum BbmdError {
    #[error("Table is full")]
    TableFull,

    #[error("Invalid format")]
    InvalidFormat,

    #[error("Entry not found")]
    NotFound,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bdt_entry_directed_broadcast() {
        let entry = BdtEntry::new(
            SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 47808),
            Ipv4Addr::new(255, 255, 255, 0),
        );

        let broadcast = entry.directed_broadcast_address();
        assert_eq!(broadcast, Ipv4Addr::new(192, 168, 1, 255));
    }

    #[test]
    fn test_bdt_add_remove() {
        let bdt = BroadcastDistributionTable::new(10);

        let entry = BdtEntry::new(
            SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 47808),
            Ipv4Addr::new(255, 255, 255, 0),
        );

        bdt.add(entry.clone()).unwrap();
        assert_eq!(bdt.len(), 1);

        bdt.remove(&entry.address);
        assert_eq!(bdt.len(), 0);
    }

    #[test]
    fn test_bdt_encode_decode() {
        let bdt = BroadcastDistributionTable::new(10);

        let entry1 = BdtEntry::new(
            SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 47808),
            Ipv4Addr::new(255, 255, 255, 0),
        );
        let entry2 = BdtEntry::new(
            SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 47808),
            Ipv4Addr::new(255, 0, 0, 0),
        );

        bdt.add(entry1).unwrap();
        bdt.add(entry2).unwrap();

        let encoded = bdt.encode();
        assert_eq!(encoded.len(), 20); // 2 entries * 10 bytes

        let bdt2 = BroadcastDistributionTable::new(10);
        bdt2.decode_and_replace(&encoded).unwrap();
        assert_eq!(bdt2.len(), 2);
    }

    #[test]
    fn test_fdt_registration() {
        let fdt = ForeignDeviceTable::new(10);

        let address = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 200), 47808);
        fdt.register(address, 60).unwrap();

        assert!(fdt.is_registered(&address));
        assert_eq!(fdt.len(), 1);
    }

    #[test]
    fn test_fdt_refresh() {
        let fdt = ForeignDeviceTable::new(10);

        let address = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 200), 47808);
        fdt.register(address, 60).unwrap();
        fdt.register(address, 120).unwrap(); // Refresh with new TTL

        assert_eq!(fdt.len(), 1); // Still just one entry
    }

    #[test]
    fn test_bbmd_register_foreign_device() {
        let config = BbmdConfig::enabled();
        let bbmd = Bbmd::new(config);

        let source = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 200), 47808);
        let data = [0x00, 0x3C]; // TTL = 60 seconds

        let msg = BvlcMessage {
            header: super::super::bvlc::BvlcHeader::new(BvlcFunction::RegisterForeignDevice, 6),
            npdu: data.to_vec(),
            original_source: None,
            result_code: None,
        };

        let response = bbmd.handle_message(&msg, source);
        assert!(response.is_some());

        let response = response.unwrap();
        assert_eq!(response.header.function, BvlcFunction::Result);
        assert_eq!(response.result_code, Some(BvlcResultCode::Success));

        assert!(bbmd.fdt().is_registered(&source));
    }

    #[test]
    fn test_bbmd_disabled() {
        let config = BbmdConfig::default(); // Disabled by default
        let bbmd = Bbmd::new(config);

        let source = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 200), 47808);
        let data = [0x00, 0x3C];

        let msg = BvlcMessage {
            header: super::super::bvlc::BvlcHeader::new(BvlcFunction::RegisterForeignDevice, 6),
            npdu: data.to_vec(),
            original_source: None,
            result_code: None,
        };

        let response = bbmd.handle_message(&msg, source);
        assert!(response.is_none()); // BBMD disabled, no response
    }

    #[test]
    fn test_forward_addresses() {
        let config = BbmdConfig::enabled();
        let bbmd = Bbmd::new(config);

        // Add BDT entry
        let bdt_addr = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 47808);
        bbmd.bdt()
            .add(BdtEntry::new(bdt_addr, Ipv4Addr::new(255, 0, 0, 0)))
            .unwrap();

        // Add FDT entry
        let fdt_addr = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 200), 47808);
        bbmd.fdt().register(fdt_addr, 60).unwrap();

        let addresses = bbmd.get_forward_addresses(None);
        assert_eq!(addresses.len(), 2);
    }
}
