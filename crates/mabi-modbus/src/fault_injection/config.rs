//! Fault injection configuration with serde support.
//!
//! Provides a unified configuration structure for defining fault injection
//! scenarios, suitable for loading from YAML/JSON configuration files.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::targeting::FaultTarget;

/// Top-level fault injection configuration.
///
/// This is the entry point for configuring all fault types via YAML/JSON.
///
/// # Example YAML
///
/// ```yaml
/// enabled: true
/// faults:
///   - type: crc_corruption
///     target:
///       unit_ids: [1, 2]
///       probability: 0.1
///     config:
///       mode: invert
///   - type: delayed_response
///     target:
///       function_codes: [0x03]
///       probability: 0.5
///     config:
///       delay_ms: 2000
///       jitter_ms: 500
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultInjectionConfig {
    /// Master enable/disable for all fault injection.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Individual fault configurations.
    #[serde(default)]
    pub faults: Vec<FaultConfig>,
}

fn default_true() -> bool {
    true
}

impl Default for FaultInjectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            faults: Vec::new(),
        }
    }
}

impl FaultInjectionConfig {
    /// Create a new empty (enabled) configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a fault configuration.
    pub fn with_fault(mut self, fault: FaultConfig) -> Self {
        self.faults.push(fault);
        self
    }

    /// Set master enabled state.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Configuration for a single fault type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultConfig {
    /// The fault type identifier.
    #[serde(rename = "type")]
    pub fault_type: FaultType,

    /// Targeting configuration.
    #[serde(default)]
    pub target: FaultTarget,

    /// Type-specific configuration.
    #[serde(default)]
    pub config: FaultTypeConfig,
}

/// Supported fault types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultType {
    /// CRC corruption (RTU).
    CrcCorruption,
    /// Wrong unit ID in response.
    WrongUnitId,
    /// Wrong function code in response.
    WrongFunctionCode,
    /// Wrong transaction ID in response (TCP).
    WrongTransactionId,
    /// Truncated response PDU.
    TruncatedResponse,
    /// Extra data appended to response.
    ExtraData,
    /// Delayed response.
    DelayedResponse,
    /// No response (silent drop).
    NoResponse,
    /// Force exception code injection.
    ExceptionInjection,
    /// Partial frame (RTU).
    PartialFrame,
}

/// Type-specific fault configuration.
///
/// Uses an internally tagged enum to allow different configuration
/// for each fault type, with sane defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FaultTypeConfig {
    // === CRC Corruption ===
    /// CRC corruption mode.
    pub crc_mode: Option<CrcCorruptionMode>,

    // === Wrong Unit ID ===
    /// How to corrupt the unit ID.
    pub unit_id_mode: Option<UnitIdCorruptionMode>,
    /// Fixed unit ID value (for Fixed mode).
    pub fixed_unit_id: Option<u8>,

    // === Wrong Function Code ===
    /// How to corrupt the function code.
    pub fc_mode: Option<FcCorruptionMode>,
    /// Fixed function code value (for Fixed mode).
    pub fixed_fc: Option<u8>,

    // === Wrong Transaction ID ===
    /// How to corrupt the transaction ID.
    pub tid_mode: Option<TidCorruptionMode>,
    /// Fixed transaction ID value (for Fixed mode).
    pub fixed_tid: Option<u16>,

    // === Truncated Response ===
    /// Truncation mode.
    pub truncation_mode: Option<TruncationMode>,
    /// Number of bytes for FixedBytes/RemoveLastN modes.
    pub truncation_bytes: Option<usize>,
    /// Percentage for Percentage mode (0.0 to 1.0).
    pub truncation_percentage: Option<f64>,

    // === Extra Data ===
    /// Extra data mode.
    pub extra_data_mode: Option<ExtraDataMode>,
    /// Specific bytes to append (for AppendBytes mode).
    pub extra_bytes: Option<Vec<u8>>,
    /// Number of random/duplicate bytes.
    pub extra_count: Option<usize>,

    // === Delayed Response ===
    /// Base delay in milliseconds.
    pub delay_ms: Option<u64>,
    /// Random jitter in milliseconds (added to base delay).
    pub jitter_ms: Option<u64>,

    // === Exception Injection ===
    /// Exception code to inject.
    pub exception_code: Option<u8>,

    // === Partial Frame ===
    /// Partial frame mode.
    pub partial_mode: Option<PartialFrameMode>,
    /// Number of bytes for FixedCount mode.
    pub partial_bytes: Option<usize>,
    /// Percentage of frame to send for Percentage mode.
    pub partial_percentage: Option<f64>,
}

impl Default for FaultTypeConfig {
    fn default() -> Self {
        Self {
            crc_mode: None,
            unit_id_mode: None,
            fixed_unit_id: None,
            fc_mode: None,
            fixed_fc: None,
            tid_mode: None,
            fixed_tid: None,
            truncation_mode: None,
            truncation_bytes: None,
            truncation_percentage: None,
            extra_data_mode: None,
            extra_bytes: None,
            extra_count: None,
            delay_ms: None,
            jitter_ms: None,
            exception_code: None,
            partial_mode: None,
            partial_bytes: None,
            partial_percentage: None,
        }
    }
}

// === Mode Enums ===

/// CRC corruption modes for RTU frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrcCorruptionMode {
    /// Set CRC to 0x0000.
    Zero,
    /// Bitwise-invert the correct CRC.
    Invert,
    /// XOR with a random value.
    RandomXor,
    /// Set CRC to a specific value.
    SetValue,
    /// Swap high/low bytes of CRC.
    SwapBytes,
}

/// Unit ID corruption modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitIdCorruptionMode {
    /// Use a fixed unit ID.
    Fixed,
    /// Increment the original unit ID by 1.
    Increment,
    /// Use a random unit ID.
    Random,
    /// Swap with a different valid unit ID.
    Swap,
}

/// Function code corruption modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FcCorruptionMode {
    /// Use a fixed function code.
    Fixed,
    /// Increment the original FC by 1.
    Increment,
    /// Use a random function code.
    Random,
    /// Swap between read (01-04) and write (05, 06, 0F, 10) FCs.
    SwapRW,
}

/// Transaction ID corruption modes (TCP).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TidCorruptionMode {
    /// Use a fixed transaction ID.
    Fixed,
    /// Increment the original TID by 1.
    Increment,
    /// Use a random transaction ID.
    Random,
    /// Swap high/low bytes.
    SwapBytes,
}

/// Response truncation modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TruncationMode {
    /// Keep only first N bytes.
    FixedBytes,
    /// Keep a percentage of the response.
    Percentage,
    /// Remove last N bytes.
    RemoveLastN,
    /// Keep only the function code byte.
    HeaderOnly,
}

/// Extra data append modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtraDataMode {
    /// Append specific bytes.
    AppendBytes,
    /// Append random bytes.
    AppendRandom,
    /// Duplicate the last N bytes.
    DuplicateLastN,
}

/// Partial frame modes (RTU).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartialFrameMode {
    /// Send exactly N bytes.
    FixedCount,
    /// Send a percentage of the frame.
    Percentage,
    /// Send up to and including the function code byte.
    UpToFc,
    /// Send up to the data section (exclude CRC).
    UpToData,
}

// === Convenience constructors for FaultConfig ===

impl FaultConfig {
    /// Create a CRC corruption fault config.
    pub fn crc_corruption(mode: CrcCorruptionMode, target: FaultTarget) -> Self {
        Self {
            fault_type: FaultType::CrcCorruption,
            target,
            config: FaultTypeConfig {
                crc_mode: Some(mode),
                ..Default::default()
            },
        }
    }

    /// Create a wrong unit ID fault config.
    pub fn wrong_unit_id(mode: UnitIdCorruptionMode, target: FaultTarget) -> Self {
        Self {
            fault_type: FaultType::WrongUnitId,
            target,
            config: FaultTypeConfig {
                unit_id_mode: Some(mode),
                ..Default::default()
            },
        }
    }

    /// Create a wrong function code fault config.
    pub fn wrong_function_code(mode: FcCorruptionMode, target: FaultTarget) -> Self {
        Self {
            fault_type: FaultType::WrongFunctionCode,
            target,
            config: FaultTypeConfig {
                fc_mode: Some(mode),
                ..Default::default()
            },
        }
    }

    /// Create a wrong transaction ID fault config.
    pub fn wrong_transaction_id(mode: TidCorruptionMode, target: FaultTarget) -> Self {
        Self {
            fault_type: FaultType::WrongTransactionId,
            target,
            config: FaultTypeConfig {
                tid_mode: Some(mode),
                ..Default::default()
            },
        }
    }

    /// Create a truncated response fault config.
    pub fn truncated_response(mode: TruncationMode, target: FaultTarget) -> Self {
        Self {
            fault_type: FaultType::TruncatedResponse,
            target,
            config: FaultTypeConfig {
                truncation_mode: Some(mode),
                ..Default::default()
            },
        }
    }

    /// Create a delayed response fault config.
    pub fn delayed_response(delay: Duration, jitter: Duration, target: FaultTarget) -> Self {
        Self {
            fault_type: FaultType::DelayedResponse,
            target,
            config: FaultTypeConfig {
                delay_ms: Some(delay.as_millis() as u64),
                jitter_ms: Some(jitter.as_millis() as u64),
                ..Default::default()
            },
        }
    }

    /// Create a no-response (drop) fault config.
    pub fn no_response(target: FaultTarget) -> Self {
        Self {
            fault_type: FaultType::NoResponse,
            target,
            config: FaultTypeConfig::default(),
        }
    }

    /// Create an exception injection fault config.
    pub fn exception_injection(exception_code: u8, target: FaultTarget) -> Self {
        Self {
            fault_type: FaultType::ExceptionInjection,
            target,
            config: FaultTypeConfig {
                exception_code: Some(exception_code),
                ..Default::default()
            },
        }
    }

    /// Create an extra data fault config.
    pub fn extra_data(mode: ExtraDataMode, count: usize, target: FaultTarget) -> Self {
        Self {
            fault_type: FaultType::ExtraData,
            target,
            config: FaultTypeConfig {
                extra_data_mode: Some(mode),
                extra_count: Some(count),
                ..Default::default()
            },
        }
    }

    /// Create a partial frame fault config (RTU).
    pub fn partial_frame(mode: PartialFrameMode, target: FaultTarget) -> Self {
        Self {
            fault_type: FaultType::PartialFrame,
            target,
            config: FaultTypeConfig {
                partial_mode: Some(mode),
                ..Default::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = FaultInjectionConfig::default();
        assert!(config.enabled);
        assert!(config.faults.is_empty());
    }

    #[test]
    fn test_config_builder() {
        let config = FaultInjectionConfig::new()
            .with_fault(FaultConfig::crc_corruption(
                CrcCorruptionMode::Invert,
                FaultTarget::new().with_probability(0.1),
            ))
            .with_fault(FaultConfig::no_response(
                FaultTarget::new()
                    .with_unit_ids(vec![1])
                    .with_probability(0.5),
            ));

        assert!(config.enabled);
        assert_eq!(config.faults.len(), 2);
        assert_eq!(config.faults[0].fault_type, FaultType::CrcCorruption);
        assert_eq!(config.faults[1].fault_type, FaultType::NoResponse);
    }

    #[test]
    fn test_fault_config_constructors() {
        let fc = FaultConfig::delayed_response(
            Duration::from_millis(1000),
            Duration::from_millis(200),
            FaultTarget::new(),
        );
        assert_eq!(fc.fault_type, FaultType::DelayedResponse);
        assert_eq!(fc.config.delay_ms, Some(1000));
        assert_eq!(fc.config.jitter_ms, Some(200));

        let fc = FaultConfig::exception_injection(0x04, FaultTarget::new());
        assert_eq!(fc.fault_type, FaultType::ExceptionInjection);
        assert_eq!(fc.config.exception_code, Some(0x04));
    }

    #[test]
    fn test_serde_roundtrip() {
        let config = FaultInjectionConfig::new()
            .with_fault(FaultConfig::crc_corruption(
                CrcCorruptionMode::Zero,
                FaultTarget::new()
                    .with_unit_ids(vec![1, 2])
                    .with_probability(0.25),
            ));

        let json = serde_json::to_string_pretty(&config).unwrap();
        let deserialized: FaultInjectionConfig = serde_json::from_str(&json).unwrap();

        assert!(deserialized.enabled);
        assert_eq!(deserialized.faults.len(), 1);
        assert_eq!(deserialized.faults[0].fault_type, FaultType::CrcCorruption);
        assert_eq!(deserialized.faults[0].target.unit_ids, vec![1, 2]);
        assert!((deserialized.faults[0].target.probability - 0.25).abs() < f64::EPSILON);
    }
}
