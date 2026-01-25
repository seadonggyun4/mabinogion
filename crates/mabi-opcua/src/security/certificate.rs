//! X.509 Certificate management for OPC UA.
//!
//! Provides certificate storage, validation, and generation capabilities.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Certificate error types.
#[derive(Debug, Error)]
pub enum CertificateError {
    #[error("Certificate not found: {0}")]
    NotFound(String),

    #[error("Certificate expired: {0}")]
    Expired(String),

    #[error("Certificate not yet valid: {0}")]
    NotYetValid(String),

    #[error("Invalid certificate: {0}")]
    Invalid(String),

    #[error("Certificate revoked: {0}")]
    Revoked(String),

    #[error("Untrusted certificate: {0}")]
    Untrusted(String),

    #[error("Certificate chain error: {0}")]
    ChainError(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Key generation error: {0}")]
    KeyGeneration(String),
}

/// Result type for certificate operations.
pub type CertificateResult<T> = Result<T, CertificateError>;

/// X.509 Certificate representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Certificate {
    /// Raw DER-encoded certificate data.
    pub der_data: Vec<u8>,
    /// Certificate thumbprint (SHA-1 hash).
    pub thumbprint: String,
    /// Subject distinguished name.
    pub subject: String,
    /// Issuer distinguished name.
    pub issuer: String,
    /// Serial number.
    pub serial_number: String,
    /// Not before date.
    pub not_before: DateTime<Utc>,
    /// Not after date.
    pub not_after: DateTime<Utc>,
    /// Subject alternative names (URIs).
    pub subject_alt_names: Vec<String>,
    /// Key usage flags.
    pub key_usage: KeyUsage,
    /// Extended key usage OIDs.
    pub extended_key_usage: Vec<String>,
    /// Is self-signed.
    pub is_self_signed: bool,
    /// Public key algorithm.
    pub public_key_algorithm: String,
    /// Public key data.
    pub public_key: Vec<u8>,
    /// Key size in bits.
    pub key_size: u32,
}

impl Certificate {
    /// Create a new certificate from DER data.
    pub fn from_der(der_data: Vec<u8>) -> CertificateResult<Self> {
        // In production, parse actual X.509 certificate
        // For simulation, create a mock certificate
        let thumbprint = compute_thumbprint(&der_data);

        Ok(Self {
            der_data,
            thumbprint,
            subject: "CN=Simulated Certificate".to_string(),
            issuer: "CN=Simulated CA".to_string(),
            serial_number: "001".to_string(),
            not_before: Utc::now() - Duration::days(1),
            not_after: Utc::now() + Duration::days(365),
            subject_alt_names: vec!["urn:trap:simulator".to_string()],
            key_usage: KeyUsage::default(),
            extended_key_usage: vec![
                "1.3.6.1.5.5.7.3.1".to_string(), // serverAuth
                "1.3.6.1.5.5.7.3.2".to_string(), // clientAuth
            ],
            is_self_signed: true,
            public_key_algorithm: "RSA".to_string(),
            public_key: vec![0u8; 256],
            key_size: 2048,
        })
    }

    /// Check if certificate is currently valid.
    pub fn is_valid(&self) -> bool {
        let now = Utc::now();
        self.not_before <= now && now <= self.not_after
    }

    /// Check if certificate is expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.not_after
    }

    /// Check if certificate is not yet valid.
    pub fn is_not_yet_valid(&self) -> bool {
        Utc::now() < self.not_before
    }

    /// Get remaining validity period.
    pub fn remaining_validity(&self) -> Option<Duration> {
        if self.is_expired() {
            None
        } else {
            Some(self.not_after - Utc::now())
        }
    }

    /// Check if certificate has specific key usage.
    pub fn has_key_usage(&self, usage: KeyUsageFlag) -> bool {
        self.key_usage.has(usage)
    }

    /// Check if suitable for OPC UA server.
    pub fn is_suitable_for_server(&self) -> bool {
        self.has_key_usage(KeyUsageFlag::DigitalSignature)
            && self.has_key_usage(KeyUsageFlag::KeyEncipherment)
            && self.has_key_usage(KeyUsageFlag::DataEncipherment)
    }

    /// Check if suitable for OPC UA client.
    pub fn is_suitable_for_client(&self) -> bool {
        self.has_key_usage(KeyUsageFlag::DigitalSignature)
    }
}

/// Key usage flags.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyUsage {
    pub digital_signature: bool,
    pub non_repudiation: bool,
    pub key_encipherment: bool,
    pub data_encipherment: bool,
    pub key_agreement: bool,
    pub key_cert_sign: bool,
    pub crl_sign: bool,
}

impl KeyUsage {
    /// Check if a specific flag is set.
    pub fn has(&self, flag: KeyUsageFlag) -> bool {
        match flag {
            KeyUsageFlag::DigitalSignature => self.digital_signature,
            KeyUsageFlag::NonRepudiation => self.non_repudiation,
            KeyUsageFlag::KeyEncipherment => self.key_encipherment,
            KeyUsageFlag::DataEncipherment => self.data_encipherment,
            KeyUsageFlag::KeyAgreement => self.key_agreement,
            KeyUsageFlag::KeyCertSign => self.key_cert_sign,
            KeyUsageFlag::CrlSign => self.crl_sign,
        }
    }

    /// Create key usage suitable for OPC UA server certificate.
    pub fn server_default() -> Self {
        Self {
            digital_signature: true,
            non_repudiation: true,
            key_encipherment: true,
            data_encipherment: true,
            key_agreement: false,
            key_cert_sign: false,
            crl_sign: false,
        }
    }
}

/// Key usage flag enumeration.
#[derive(Debug, Clone, Copy)]
pub enum KeyUsageFlag {
    DigitalSignature,
    NonRepudiation,
    KeyEncipherment,
    DataEncipherment,
    KeyAgreement,
    KeyCertSign,
    CrlSign,
}

/// Certificate store type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertificateStoreType {
    /// Trusted certificates (CA and peers).
    Trusted,
    /// Rejected certificates.
    Rejected,
    /// Issuer certificates (CA chain).
    Issuer,
    /// Own certificates.
    Own,
}

/// Certificate store for managing certificates.
#[derive(Debug)]
pub struct CertificateStore {
    store_type: CertificateStoreType,
    path: PathBuf,
    certificates: DashMap<String, Certificate>,
}

impl CertificateStore {
    /// Create a new certificate store.
    pub fn new(store_type: CertificateStoreType, path: PathBuf) -> Self {
        Self {
            store_type,
            path,
            certificates: DashMap::new(),
        }
    }

    /// Get the store type.
    pub fn store_type(&self) -> CertificateStoreType {
        self.store_type
    }

    /// Get the store path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Add a certificate to the store.
    pub fn add(&self, certificate: Certificate) -> CertificateResult<()> {
        let thumbprint = certificate.thumbprint.clone();
        self.certificates.insert(thumbprint.clone(), certificate);

        debug!(thumbprint = %thumbprint, store = ?self.store_type, "Certificate added to store");
        Ok(())
    }

    /// Remove a certificate from the store.
    pub fn remove(&self, thumbprint: &str) -> Option<Certificate> {
        self.certificates.remove(thumbprint).map(|(_, cert)| cert)
    }

    /// Get a certificate by thumbprint.
    pub fn get(&self, thumbprint: &str) -> Option<Certificate> {
        self.certificates.get(thumbprint).map(|c| c.clone())
    }

    /// Check if a certificate exists.
    pub fn contains(&self, thumbprint: &str) -> bool {
        self.certificates.contains_key(thumbprint)
    }

    /// Get all certificates.
    pub fn all(&self) -> Vec<Certificate> {
        self.certificates.iter().map(|e| e.value().clone()).collect()
    }

    /// Get certificate count.
    pub fn count(&self) -> usize {
        self.certificates.len()
    }

    /// Clear all certificates.
    pub fn clear(&self) {
        self.certificates.clear();
    }

    /// Find certificate by subject.
    pub fn find_by_subject(&self, subject: &str) -> Option<Certificate> {
        self.certificates
            .iter()
            .find(|e| e.value().subject.contains(subject))
            .map(|e| e.value().clone())
    }

    /// Find certificate by URI.
    pub fn find_by_uri(&self, uri: &str) -> Option<Certificate> {
        self.certificates
            .iter()
            .find(|e| e.value().subject_alt_names.contains(&uri.to_string()))
            .map(|e| e.value().clone())
    }
}

/// Certificate validation result.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Is the certificate valid.
    pub is_valid: bool,
    /// Validation status code.
    pub status: ValidationStatus,
    /// Error message if invalid.
    pub error_message: Option<String>,
    /// Certificate chain (if validated).
    pub chain: Vec<Certificate>,
}

impl ValidationResult {
    /// Create a valid result.
    pub fn valid(chain: Vec<Certificate>) -> Self {
        Self {
            is_valid: true,
            status: ValidationStatus::Good,
            error_message: None,
            chain,
        }
    }

    /// Create an invalid result.
    pub fn invalid(status: ValidationStatus, message: impl Into<String>) -> Self {
        Self {
            is_valid: false,
            status,
            error_message: Some(message.into()),
            chain: Vec::new(),
        }
    }
}

/// Certificate validation status codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationStatus {
    /// Certificate is valid.
    Good,
    /// Certificate is expired.
    Expired,
    /// Certificate is not yet valid.
    NotYetValid,
    /// Certificate is revoked.
    Revoked,
    /// Certificate is untrusted.
    Untrusted,
    /// Certificate chain is incomplete.
    ChainIncomplete,
    /// Signature verification failed.
    SignatureInvalid,
    /// Key usage is invalid.
    KeyUsageInvalid,
    /// Certificate is self-signed but not trusted.
    SelfSignedNotTrusted,
    /// Unknown error.
    Unknown,
}

/// Certificate validator configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateValidatorConfig {
    /// Check certificate expiration.
    pub check_time_validity: bool,
    /// Check certificate chain.
    pub check_chain: bool,
    /// Check revocation status.
    pub check_revocation: bool,
    /// Allow self-signed certificates.
    pub allow_self_signed: bool,
    /// Minimum key size required.
    pub minimum_key_size: u32,
    /// Accepted certificate purposes.
    pub accepted_purposes: Vec<String>,
}

impl Default for CertificateValidatorConfig {
    fn default() -> Self {
        Self {
            check_time_validity: true,
            check_chain: true,
            check_revocation: false, // Typically disabled for industrial systems
            allow_self_signed: true,
            minimum_key_size: 2048,
            accepted_purposes: vec![
                "1.3.6.1.5.5.7.3.1".to_string(), // serverAuth
                "1.3.6.1.5.5.7.3.2".to_string(), // clientAuth
            ],
        }
    }
}

/// Certificate validator.
pub struct CertificateValidator {
    config: CertificateValidatorConfig,
    trusted_store: Arc<CertificateStore>,
    issuer_store: Arc<CertificateStore>,
}

impl CertificateValidator {
    /// Create a new validator.
    pub fn new(
        config: CertificateValidatorConfig,
        trusted_store: Arc<CertificateStore>,
        issuer_store: Arc<CertificateStore>,
    ) -> Self {
        Self {
            config,
            trusted_store,
            issuer_store,
        }
    }

    /// Validate a certificate.
    pub fn validate(&self, certificate: &Certificate) -> ValidationResult {
        // Check time validity
        if self.config.check_time_validity {
            if certificate.is_expired() {
                return ValidationResult::invalid(
                    ValidationStatus::Expired,
                    format!("Certificate expired on {}", certificate.not_after),
                );
            }

            if certificate.is_not_yet_valid() {
                return ValidationResult::invalid(
                    ValidationStatus::NotYetValid,
                    format!("Certificate not valid until {}", certificate.not_before),
                );
            }
        }

        // Check key size
        if certificate.key_size < self.config.minimum_key_size {
            return ValidationResult::invalid(
                ValidationStatus::KeyUsageInvalid,
                format!(
                    "Key size {} is less than minimum {}",
                    certificate.key_size, self.config.minimum_key_size
                ),
            );
        }

        // Check if trusted
        if self.trusted_store.contains(&certificate.thumbprint) {
            return ValidationResult::valid(vec![certificate.clone()]);
        }

        // Check self-signed
        if certificate.is_self_signed {
            if self.config.allow_self_signed {
                return ValidationResult::valid(vec![certificate.clone()]);
            } else {
                return ValidationResult::invalid(
                    ValidationStatus::SelfSignedNotTrusted,
                    "Self-signed certificates are not trusted",
                );
            }
        }

        // Check certificate chain
        if self.config.check_chain {
            match self.validate_chain(certificate) {
                Ok(chain) => return ValidationResult::valid(chain),
                Err(status) => {
                    return ValidationResult::invalid(status, "Certificate chain validation failed");
                }
            }
        }

        ValidationResult::invalid(ValidationStatus::Untrusted, "Certificate is not trusted")
    }

    /// Validate certificate chain.
    fn validate_chain(&self, certificate: &Certificate) -> Result<Vec<Certificate>, ValidationStatus> {
        let mut chain = vec![certificate.clone()];

        // Find issuer certificate
        if let Some(issuer) = self.issuer_store.find_by_subject(&certificate.issuer) {
            chain.push(issuer.clone());

            // Check if issuer is trusted
            if self.trusted_store.contains(&issuer.thumbprint) {
                return Ok(chain);
            }

            // Recursively validate issuer
            if !issuer.is_self_signed {
                match self.validate_chain(&issuer) {
                    Ok(mut issuer_chain) => {
                        chain.append(&mut issuer_chain);
                        return Ok(chain);
                    }
                    Err(status) => return Err(status),
                }
            }
        }

        Err(ValidationStatus::ChainIncomplete)
    }
}

/// Certificate manager configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateManagerConfig {
    /// PKI directory path.
    pub pki_path: PathBuf,
    /// Own certificate path.
    pub certificate_path: Option<PathBuf>,
    /// Private key path.
    pub private_key_path: Option<PathBuf>,
    /// Validator configuration.
    pub validator_config: CertificateValidatorConfig,
    /// Auto-generate self-signed certificate if none exists.
    pub auto_generate: bool,
    /// Certificate validity period in days (for auto-generation).
    pub validity_days: u32,
    /// Key size for auto-generation.
    pub key_size: u32,
}

impl Default for CertificateManagerConfig {
    fn default() -> Self {
        Self {
            pki_path: PathBuf::from("./pki"),
            certificate_path: None,
            private_key_path: None,
            validator_config: CertificateValidatorConfig::default(),
            auto_generate: true,
            validity_days: 365,
            key_size: 2048,
        }
    }
}

/// Certificate manager for OPC UA security.
pub struct CertificateManager {
    config: CertificateManagerConfig,
    own_certificate: RwLock<Option<Certificate>>,
    private_key: RwLock<Option<Vec<u8>>>,
    trusted_store: Arc<CertificateStore>,
    rejected_store: Arc<CertificateStore>,
    issuer_store: Arc<CertificateStore>,
    validator: CertificateValidator,
}

impl CertificateManager {
    /// Create a new certificate manager.
    pub fn new(config: CertificateManagerConfig) -> Self {
        let trusted_store = Arc::new(CertificateStore::new(
            CertificateStoreType::Trusted,
            config.pki_path.join("trusted"),
        ));

        let rejected_store = Arc::new(CertificateStore::new(
            CertificateStoreType::Rejected,
            config.pki_path.join("rejected"),
        ));

        let issuer_store = Arc::new(CertificateStore::new(
            CertificateStoreType::Issuer,
            config.pki_path.join("issuers"),
        ));

        let validator = CertificateValidator::new(
            config.validator_config.clone(),
            trusted_store.clone(),
            issuer_store.clone(),
        );

        Self {
            config,
            own_certificate: RwLock::new(None),
            private_key: RwLock::new(None),
            trusted_store,
            rejected_store,
            issuer_store,
            validator,
        }
    }

    /// Initialize the certificate manager.
    pub fn initialize(&self) -> CertificateResult<()> {
        // Try to load own certificate
        if let Some(ref cert_path) = self.config.certificate_path {
            if cert_path.exists() {
                // Load certificate from file
                info!(path = ?cert_path, "Loading server certificate");
                // In production, actually load the certificate
            }
        }

        // Auto-generate if needed
        if self.own_certificate.read().is_none() && self.config.auto_generate {
            info!("Generating self-signed certificate");
            let (cert, key) = self.generate_self_signed()?;
            *self.own_certificate.write() = Some(cert);
            *self.private_key.write() = Some(key);
        }

        Ok(())
    }

    /// Generate a self-signed certificate.
    pub fn generate_self_signed(&self) -> CertificateResult<(Certificate, Vec<u8>)> {
        // Generate key pair (simulated)
        let private_key = vec![0x42u8; self.config.key_size as usize / 8];
        let public_key = vec![0x43u8; self.config.key_size as usize / 8];

        // Create certificate
        let now = Utc::now();
        let certificate = Certificate {
            der_data: vec![0x30, 0x82], // ASN.1 SEQUENCE prefix (simulated)
            thumbprint: compute_thumbprint(&public_key),
            subject: "CN=TRAP Simulator, O=Trap, C=KR".to_string(),
            issuer: "CN=TRAP Simulator, O=Trap, C=KR".to_string(),
            serial_number: format!("{:016X}", now.timestamp_millis()),
            not_before: now,
            not_after: now + Duration::days(self.config.validity_days as i64),
            subject_alt_names: vec![
                "urn:trap:simulator:opcua".to_string(),
                "localhost".to_string(),
            ],
            key_usage: KeyUsage::server_default(),
            extended_key_usage: vec![
                "1.3.6.1.5.5.7.3.1".to_string(),
                "1.3.6.1.5.5.7.3.2".to_string(),
            ],
            is_self_signed: true,
            public_key_algorithm: "RSA".to_string(),
            public_key,
            key_size: self.config.key_size,
        };

        info!(
            thumbprint = %certificate.thumbprint,
            subject = %certificate.subject,
            "Generated self-signed certificate"
        );

        Ok((certificate, private_key))
    }

    /// Get own certificate.
    pub fn own_certificate(&self) -> Option<Certificate> {
        self.own_certificate.read().clone()
    }

    /// Get private key.
    pub fn private_key(&self) -> Option<Vec<u8>> {
        self.private_key.read().clone()
    }

    /// Validate a peer certificate.
    pub fn validate_certificate(&self, certificate: &Certificate) -> ValidationResult {
        self.validator.validate(certificate)
    }

    /// Trust a certificate.
    pub fn trust_certificate(&self, certificate: Certificate) -> CertificateResult<()> {
        // Remove from rejected if present
        self.rejected_store.remove(&certificate.thumbprint);

        // Add to trusted
        self.trusted_store.add(certificate)?;

        Ok(())
    }

    /// Reject a certificate.
    pub fn reject_certificate(&self, certificate: Certificate) -> CertificateResult<()> {
        // Remove from trusted if present
        self.trusted_store.remove(&certificate.thumbprint);

        // Add to rejected
        self.rejected_store.add(certificate)?;

        Ok(())
    }

    /// Get trusted certificates.
    pub fn trusted_certificates(&self) -> Vec<Certificate> {
        self.trusted_store.all()
    }

    /// Get rejected certificates.
    pub fn rejected_certificates(&self) -> Vec<Certificate> {
        self.rejected_store.all()
    }

    /// Get certificate store.
    pub fn trusted_store(&self) -> &Arc<CertificateStore> {
        &self.trusted_store
    }

    /// Get rejected store.
    pub fn rejected_store(&self) -> &Arc<CertificateStore> {
        &self.rejected_store
    }
}

/// Compute certificate thumbprint (SHA-1 hash).
fn compute_thumbprint(data: &[u8]) -> String {
    // Simple hash for simulation
    let mut hash = [0u8; 20];
    for (i, &byte) in data.iter().enumerate() {
        hash[i % 20] = hash[i % 20].wrapping_add(byte);
    }

    hash.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_certificate_from_der() {
        let der_data = vec![0x30, 0x82, 0x01, 0x02, 0x03];
        let cert = Certificate::from_der(der_data).unwrap();

        assert!(!cert.thumbprint.is_empty());
        assert!(cert.is_valid());
    }

    #[test]
    fn test_certificate_validity() {
        let mut cert = Certificate::from_der(vec![0x30]).unwrap();

        // Valid certificate
        assert!(cert.is_valid());
        assert!(!cert.is_expired());
        assert!(!cert.is_not_yet_valid());

        // Expired certificate
        cert.not_after = Utc::now() - Duration::days(1);
        assert!(cert.is_expired());
        assert!(!cert.is_valid());

        // Not yet valid certificate
        cert.not_after = Utc::now() + Duration::days(365);
        cert.not_before = Utc::now() + Duration::days(1);
        assert!(cert.is_not_yet_valid());
        assert!(!cert.is_valid());
    }

    #[test]
    fn test_certificate_store() {
        let store = CertificateStore::new(
            CertificateStoreType::Trusted,
            PathBuf::from("/tmp/test"),
        );

        let cert = Certificate::from_der(vec![0x30, 0x01]).unwrap();
        let thumbprint = cert.thumbprint.clone();

        store.add(cert.clone()).unwrap();
        assert!(store.contains(&thumbprint));
        assert_eq!(store.count(), 1);

        let retrieved = store.get(&thumbprint).unwrap();
        assert_eq!(retrieved.thumbprint, thumbprint);

        store.remove(&thumbprint);
        assert!(!store.contains(&thumbprint));
    }

    #[test]
    fn test_certificate_manager() {
        let config = CertificateManagerConfig {
            auto_generate: true,
            ..Default::default()
        };

        let manager = CertificateManager::new(config);
        manager.initialize().unwrap();

        let cert = manager.own_certificate().unwrap();
        assert!(cert.is_self_signed);
        assert!(cert.is_valid());

        let key = manager.private_key().unwrap();
        assert!(!key.is_empty());
    }

    #[test]
    fn test_validation() {
        let config = CertificateManagerConfig::default();
        let manager = CertificateManager::new(config);
        manager.initialize().unwrap();

        let cert = manager.own_certificate().unwrap();
        let result = manager.validate_certificate(&cert);

        assert!(result.is_valid);
        assert_eq!(result.status, ValidationStatus::Good);
    }

    #[test]
    fn test_trust_reject() {
        let manager = CertificateManager::new(CertificateManagerConfig::default());

        let cert = Certificate::from_der(vec![0x30, 0x82, 0x01]).unwrap();
        let thumbprint = cert.thumbprint.clone();

        // Trust certificate
        manager.trust_certificate(cert.clone()).unwrap();
        assert!(manager.trusted_store.contains(&thumbprint));

        // Reject certificate (should move from trusted to rejected)
        manager.reject_certificate(cert).unwrap();
        assert!(!manager.trusted_store.contains(&thumbprint));
        assert!(manager.rejected_store.contains(&thumbprint));
    }
}
