//! Cryptographic operations for OPC UA security.
//!
//! Provides encryption, decryption, signing, and hashing operations
//! using pluggable algorithm implementations.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, trace, warn};

use crate::config::SecurityPolicy;
use super::policy::SecurityPolicyConfig;

/// Cryptographic error types.
#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("Signing failed: {0}")]
    SigningFailed(String),

    #[error("Verification failed: {0}")]
    VerificationFailed(String),

    #[error("Invalid key: {0}")]
    InvalidKey(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Unsupported algorithm: {0}")]
    UnsupportedAlgorithm(String),

    #[error("Key generation failed: {0}")]
    KeyGenerationFailed(String),
}

/// Result type for cryptographic operations.
pub type CryptoResult<T> = Result<T, CryptoError>;

/// Symmetric encryption algorithm identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymmetricAlgorithm {
    /// No encryption.
    None,
    /// AES-128-CBC.
    Aes128Cbc,
    /// AES-256-CBC.
    Aes256Cbc,
}

impl SymmetricAlgorithm {
    /// Get the key length in bytes.
    pub fn key_length(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Aes128Cbc => 16,
            Self::Aes256Cbc => 32,
        }
    }

    /// Get the block size in bytes.
    pub fn block_size(&self) -> usize {
        match self {
            Self::None => 0,
            Self::Aes128Cbc | Self::Aes256Cbc => 16,
        }
    }

    /// Get the IV length in bytes.
    pub fn iv_length(&self) -> usize {
        self.block_size()
    }
}

/// Asymmetric encryption algorithm identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsymmetricAlgorithm {
    /// No encryption.
    None,
    /// RSA PKCS#1 v1.5.
    RsaPkcs1,
    /// RSA OAEP with SHA-1.
    RsaOaepSha1,
    /// RSA OAEP with SHA-256.
    RsaOaepSha256,
    /// RSA PSS with SHA-256.
    RsaPssSha256,
}

/// Hash algorithm identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    /// SHA-1 (deprecated).
    Sha1,
    /// SHA-256.
    Sha256,
    /// SHA-384.
    Sha384,
    /// SHA-512.
    Sha512,
}

impl HashAlgorithm {
    /// Get the output length in bytes.
    pub fn output_length(&self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }
}

/// Result of an encryption operation.
#[derive(Debug, Clone)]
pub struct EncryptionResult {
    /// Encrypted data.
    pub ciphertext: Vec<u8>,
    /// Initialization vector (for symmetric encryption).
    pub iv: Option<Vec<u8>>,
}

/// Result of a decryption operation.
#[derive(Debug, Clone)]
pub struct DecryptionResult {
    /// Decrypted plaintext.
    pub plaintext: Vec<u8>,
}

/// Result of a signing operation.
#[derive(Debug, Clone)]
pub struct SignatureResult {
    /// Digital signature.
    pub signature: Vec<u8>,
}

/// Crypto provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoProviderConfig {
    /// Enable hardware acceleration if available.
    pub enable_hw_acceleration: bool,
    /// Random number generator seed (for testing).
    pub rng_seed: Option<u64>,
}

impl Default for CryptoProviderConfig {
    fn default() -> Self {
        Self {
            enable_hw_acceleration: true,
            rng_seed: None,
        }
    }
}

/// Key material for cryptographic operations.
#[derive(Debug, Clone)]
pub struct KeyMaterial {
    /// Signing key.
    pub signing_key: Vec<u8>,
    /// Encryption key.
    pub encrypting_key: Vec<u8>,
    /// Initialization vector.
    pub iv: Vec<u8>,
}

impl KeyMaterial {
    /// Create empty key material.
    pub fn empty() -> Self {
        Self {
            signing_key: Vec::new(),
            encrypting_key: Vec::new(),
            iv: Vec::new(),
        }
    }

    /// Check if key material is valid for the given policy.
    pub fn is_valid_for(&self, policy: &SecurityPolicyConfig) -> bool {
        if policy.policy == SecurityPolicy::None {
            return true;
        }

        self.signing_key.len() >= policy.derived_signature_key_length as usize
            && self.encrypting_key.len() >= policy.symmetric_key_length as usize
            && self.iv.len() >= policy.symmetric_block_size as usize
    }
}

/// Cryptographic provider for OPC UA security operations.
///
/// This is a pluggable interface that can be backed by different
/// cryptographic libraries (ring, openssl, etc.).
pub struct CryptoProvider {
    config: CryptoProviderConfig,
    policy_config: SecurityPolicyConfig,
}

impl CryptoProvider {
    /// Create a new crypto provider with default config.
    pub fn new(policy: SecurityPolicy) -> Self {
        Self {
            config: CryptoProviderConfig::default(),
            policy_config: SecurityPolicyConfig::for_policy(policy),
        }
    }

    /// Create with custom config.
    pub fn with_config(policy: SecurityPolicy, config: CryptoProviderConfig) -> Self {
        Self {
            config,
            policy_config: SecurityPolicyConfig::for_policy(policy),
        }
    }

    /// Get the security policy.
    pub fn policy(&self) -> SecurityPolicy {
        self.policy_config.policy
    }

    /// Get the policy configuration.
    pub fn policy_config(&self) -> &SecurityPolicyConfig {
        &self.policy_config
    }

    // =========================================================================
    // Symmetric Operations
    // =========================================================================

    /// Encrypt data using symmetric encryption.
    pub fn symmetric_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8],
        iv: &[u8],
    ) -> CryptoResult<EncryptionResult> {
        if self.policy_config.policy == SecurityPolicy::None {
            return Ok(EncryptionResult {
                ciphertext: plaintext.to_vec(),
                iv: None,
            });
        }

        // Validate key and IV lengths
        let expected_key_len = self.policy_config.symmetric_key_length as usize;
        let expected_iv_len = self.policy_config.symmetric_block_size as usize;

        if key.len() != expected_key_len {
            return Err(CryptoError::InvalidKey(format!(
                "Expected key length {}, got {}",
                expected_key_len,
                key.len()
            )));
        }

        if iv.len() != expected_iv_len {
            return Err(CryptoError::InvalidData(format!(
                "Expected IV length {}, got {}",
                expected_iv_len,
                iv.len()
            )));
        }

        // Apply PKCS7 padding
        let block_size = self.policy_config.symmetric_block_size as usize;
        let padded = pkcs7_pad(plaintext, block_size);

        // Simulate AES-CBC encryption
        // In production, this would use a real crypto library
        let ciphertext = simulate_aes_cbc_encrypt(&padded, key, iv);

        trace!(
            plaintext_len = plaintext.len(),
            ciphertext_len = ciphertext.len(),
            "Symmetric encryption completed"
        );

        Ok(EncryptionResult {
            ciphertext,
            iv: Some(iv.to_vec()),
        })
    }

    /// Decrypt data using symmetric encryption.
    pub fn symmetric_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8],
        iv: &[u8],
    ) -> CryptoResult<DecryptionResult> {
        if self.policy_config.policy == SecurityPolicy::None {
            return Ok(DecryptionResult {
                plaintext: ciphertext.to_vec(),
            });
        }

        // Validate inputs
        let block_size = self.policy_config.symmetric_block_size as usize;
        if ciphertext.len() % block_size != 0 {
            return Err(CryptoError::InvalidData(
                "Ciphertext length must be multiple of block size".to_string(),
            ));
        }

        // Simulate AES-CBC decryption
        let decrypted = simulate_aes_cbc_decrypt(ciphertext, key, iv);

        // Remove PKCS7 padding
        let plaintext = pkcs7_unpad(&decrypted)
            .map_err(|e| CryptoError::DecryptionFailed(e))?;

        trace!(
            ciphertext_len = ciphertext.len(),
            plaintext_len = plaintext.len(),
            "Symmetric decryption completed"
        );

        Ok(DecryptionResult { plaintext })
    }

    // =========================================================================
    // Asymmetric Operations
    // =========================================================================

    /// Encrypt data using asymmetric encryption (RSA).
    pub fn asymmetric_encrypt(
        &self,
        plaintext: &[u8],
        public_key: &[u8],
    ) -> CryptoResult<EncryptionResult> {
        if self.policy_config.policy == SecurityPolicy::None {
            return Ok(EncryptionResult {
                ciphertext: plaintext.to_vec(),
                iv: None,
            });
        }

        // Validate public key
        if public_key.is_empty() {
            return Err(CryptoError::InvalidKey("Empty public key".to_string()));
        }

        // Simulate RSA encryption
        // In production, would use actual RSA implementation
        let ciphertext = simulate_rsa_encrypt(plaintext, public_key);

        debug!(
            plaintext_len = plaintext.len(),
            ciphertext_len = ciphertext.len(),
            "Asymmetric encryption completed"
        );

        Ok(EncryptionResult {
            ciphertext,
            iv: None,
        })
    }

    /// Decrypt data using asymmetric encryption (RSA).
    pub fn asymmetric_decrypt(
        &self,
        ciphertext: &[u8],
        private_key: &[u8],
    ) -> CryptoResult<DecryptionResult> {
        if self.policy_config.policy == SecurityPolicy::None {
            return Ok(DecryptionResult {
                plaintext: ciphertext.to_vec(),
            });
        }

        // Validate private key
        if private_key.is_empty() {
            return Err(CryptoError::InvalidKey("Empty private key".to_string()));
        }

        // Simulate RSA decryption
        let plaintext = simulate_rsa_decrypt(ciphertext, private_key);

        debug!(
            ciphertext_len = ciphertext.len(),
            plaintext_len = plaintext.len(),
            "Asymmetric decryption completed"
        );

        Ok(DecryptionResult { plaintext })
    }

    // =========================================================================
    // Signing Operations
    // =========================================================================

    /// Sign data using HMAC (symmetric).
    pub fn hmac_sign(&self, data: &[u8], key: &[u8]) -> CryptoResult<SignatureResult> {
        if self.policy_config.policy == SecurityPolicy::None {
            return Ok(SignatureResult {
                signature: Vec::new(),
            });
        }

        // Simulate HMAC-SHA256
        let signature = simulate_hmac_sha256(data, key);

        trace!(data_len = data.len(), sig_len = signature.len(), "HMAC sign completed");

        Ok(SignatureResult { signature })
    }

    /// Verify HMAC signature.
    pub fn hmac_verify(&self, data: &[u8], key: &[u8], signature: &[u8]) -> CryptoResult<bool> {
        if self.policy_config.policy == SecurityPolicy::None {
            return Ok(true);
        }

        let expected = simulate_hmac_sha256(data, key);
        let valid = constant_time_compare(&expected, signature);

        trace!(valid, "HMAC verify completed");

        Ok(valid)
    }

    /// Sign data using RSA.
    pub fn rsa_sign(&self, data: &[u8], private_key: &[u8]) -> CryptoResult<SignatureResult> {
        if self.policy_config.policy == SecurityPolicy::None {
            return Ok(SignatureResult {
                signature: Vec::new(),
            });
        }

        // Simulate RSA signature
        let signature = simulate_rsa_sign(data, private_key);

        debug!(data_len = data.len(), sig_len = signature.len(), "RSA sign completed");

        Ok(SignatureResult { signature })
    }

    /// Verify RSA signature.
    pub fn rsa_verify(
        &self,
        data: &[u8],
        public_key: &[u8],
        signature: &[u8],
    ) -> CryptoResult<bool> {
        if self.policy_config.policy == SecurityPolicy::None {
            return Ok(true);
        }

        // Simulate RSA verification
        let valid = simulate_rsa_verify(data, public_key, signature);

        debug!(valid, "RSA verify completed");

        Ok(valid)
    }

    // =========================================================================
    // Hashing Operations
    // =========================================================================

    /// Compute hash of data.
    pub fn hash(&self, algorithm: HashAlgorithm, data: &[u8]) -> Vec<u8> {
        match algorithm {
            HashAlgorithm::Sha1 => simulate_sha1(data),
            HashAlgorithm::Sha256 => simulate_sha256(data),
            HashAlgorithm::Sha384 => simulate_sha384(data),
            HashAlgorithm::Sha512 => simulate_sha512(data),
        }
    }

    /// Compute SHA-256 hash.
    pub fn sha256(&self, data: &[u8]) -> Vec<u8> {
        self.hash(HashAlgorithm::Sha256, data)
    }

    // =========================================================================
    // Key Derivation
    // =========================================================================

    /// Derive keys from shared secret using P_SHA256.
    pub fn derive_keys(
        &self,
        secret: &[u8],
        seed: &[u8],
        signing_key_length: usize,
        encrypting_key_length: usize,
        iv_length: usize,
    ) -> CryptoResult<KeyMaterial> {
        if self.policy_config.policy == SecurityPolicy::None {
            return Ok(KeyMaterial::empty());
        }

        // Derive key material using P_SHA256
        let total_length = signing_key_length + encrypting_key_length + iv_length;
        let derived = p_sha256(secret, seed, total_length);

        let signing_key = derived[..signing_key_length].to_vec();
        let encrypting_key = derived[signing_key_length..signing_key_length + encrypting_key_length].to_vec();
        let iv = derived[signing_key_length + encrypting_key_length..].to_vec();

        debug!(
            signing_key_len = signing_key.len(),
            encrypting_key_len = encrypting_key.len(),
            iv_len = iv.len(),
            "Key derivation completed"
        );

        Ok(KeyMaterial {
            signing_key,
            encrypting_key,
            iv,
        })
    }

    // =========================================================================
    // Random Number Generation
    // =========================================================================

    /// Generate random bytes.
    pub fn random_bytes(&self, length: usize) -> Vec<u8> {
        if let Some(seed) = self.config.rng_seed {
            // Deterministic for testing
            deterministic_random(seed, length)
        } else {
            // True random
            secure_random(length)
        }
    }

    /// Generate a random nonce.
    pub fn generate_nonce(&self) -> Vec<u8> {
        self.random_bytes(self.policy_config.secure_channel_nonce_length as usize)
    }
}

// =========================================================================
// Padding Functions
// =========================================================================

/// Apply PKCS7 padding.
fn pkcs7_pad(data: &[u8], block_size: usize) -> Vec<u8> {
    let padding_len = block_size - (data.len() % block_size);
    let mut padded = data.to_vec();
    padded.extend(std::iter::repeat(padding_len as u8).take(padding_len));
    padded
}

/// Remove PKCS7 padding.
fn pkcs7_unpad(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.is_empty() {
        return Err("Empty data".to_string());
    }

    let padding_len = *data.last().unwrap() as usize;
    if padding_len == 0 || padding_len > data.len() || padding_len > 16 {
        return Err("Invalid padding".to_string());
    }

    // Verify padding bytes
    for &byte in &data[data.len() - padding_len..] {
        if byte != padding_len as u8 {
            return Err("Invalid padding bytes".to_string());
        }
    }

    Ok(data[..data.len() - padding_len].to_vec())
}

/// Constant-time comparison to prevent timing attacks.
fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

// =========================================================================
// Simulation Functions (Replace with real crypto in production)
// =========================================================================

/// Simulate AES-CBC encryption.
fn simulate_aes_cbc_encrypt(plaintext: &[u8], key: &[u8], iv: &[u8]) -> Vec<u8> {
    // XOR-based simulation for structure (NOT SECURE - use real AES in production)
    let mut ciphertext = Vec::with_capacity(plaintext.len());
    let block_size = 16;

    let mut prev_block = iv.to_vec();
    for chunk in plaintext.chunks(block_size) {
        let mut block = vec![0u8; block_size];
        for (i, &byte) in chunk.iter().enumerate() {
            block[i] = byte ^ prev_block[i] ^ key[i % key.len()];
        }
        prev_block = block.clone();
        ciphertext.extend(block);
    }

    ciphertext
}

/// Simulate AES-CBC decryption.
fn simulate_aes_cbc_decrypt(ciphertext: &[u8], key: &[u8], iv: &[u8]) -> Vec<u8> {
    let mut plaintext = Vec::with_capacity(ciphertext.len());
    let block_size = 16;

    let mut prev_block = iv.to_vec();
    for chunk in ciphertext.chunks(block_size) {
        let mut block = vec![0u8; block_size];
        for (i, &byte) in chunk.iter().enumerate() {
            block[i] = byte ^ prev_block[i] ^ key[i % key.len()];
        }
        prev_block = chunk.to_vec();
        plaintext.extend(block);
    }

    plaintext
}

/// Simulate RSA encryption.
fn simulate_rsa_encrypt(plaintext: &[u8], public_key: &[u8]) -> Vec<u8> {
    // Simple XOR simulation (NOT SECURE)
    plaintext
        .iter()
        .enumerate()
        .map(|(i, &b)| b ^ public_key[i % public_key.len()])
        .collect()
}

/// Simulate RSA decryption.
fn simulate_rsa_decrypt(ciphertext: &[u8], private_key: &[u8]) -> Vec<u8> {
    // Simple XOR simulation (NOT SECURE)
    ciphertext
        .iter()
        .enumerate()
        .map(|(i, &b)| b ^ private_key[i % private_key.len()])
        .collect()
}

/// Simulate HMAC-SHA256.
fn simulate_hmac_sha256(data: &[u8], key: &[u8]) -> Vec<u8> {
    // Simple hash simulation
    let mut hash = vec![0u8; 32];
    for (i, &b) in data.iter().enumerate() {
        hash[i % 32] ^= b ^ key[i % key.len()];
    }
    hash
}

/// Simulate RSA signing.
fn simulate_rsa_sign(data: &[u8], private_key: &[u8]) -> Vec<u8> {
    simulate_hmac_sha256(data, private_key)
}

/// Simulate RSA verification.
fn simulate_rsa_verify(data: &[u8], public_key: &[u8], signature: &[u8]) -> bool {
    let expected = simulate_hmac_sha256(data, public_key);
    constant_time_compare(&expected, signature)
}

/// Simulate SHA-1.
fn simulate_sha1(data: &[u8]) -> Vec<u8> {
    let mut hash = vec![0u8; 20];
    for (i, &b) in data.iter().enumerate() {
        hash[i % 20] = hash[i % 20].wrapping_add(b);
    }
    hash
}

/// Simulate SHA-256.
fn simulate_sha256(data: &[u8]) -> Vec<u8> {
    let mut hash = vec![0u8; 32];
    for (i, &b) in data.iter().enumerate() {
        hash[i % 32] = hash[i % 32].wrapping_add(b);
    }
    hash
}

/// Simulate SHA-384.
fn simulate_sha384(data: &[u8]) -> Vec<u8> {
    let mut hash = vec![0u8; 48];
    for (i, &b) in data.iter().enumerate() {
        hash[i % 48] = hash[i % 48].wrapping_add(b);
    }
    hash
}

/// Simulate SHA-512.
fn simulate_sha512(data: &[u8]) -> Vec<u8> {
    let mut hash = vec![0u8; 64];
    for (i, &b) in data.iter().enumerate() {
        hash[i % 64] = hash[i % 64].wrapping_add(b);
    }
    hash
}

/// P_SHA256 key derivation function.
fn p_sha256(secret: &[u8], seed: &[u8], length: usize) -> Vec<u8> {
    let mut result = Vec::with_capacity(length);
    let mut a = seed.to_vec();

    while result.len() < length {
        a = simulate_hmac_sha256(&a, secret);
        let mut combined = a.clone();
        combined.extend_from_slice(seed);
        let p = simulate_hmac_sha256(&combined, secret);
        result.extend(p);
    }

    result.truncate(length);
    result
}

/// Generate deterministic random bytes for testing.
fn deterministic_random(seed: u64, length: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(length);
    let mut state = seed;

    for _ in 0..length {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        bytes.push((state >> 56) as u8);
    }

    bytes
}

/// Generate secure random bytes.
fn secure_random(length: usize) -> Vec<u8> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    deterministic_random(seed, length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkcs7_padding() {
        let data = b"hello";
        let padded = pkcs7_pad(data, 16);
        assert_eq!(padded.len(), 16);
        assert_eq!(padded[5..], [11u8; 11]);

        let unpadded = pkcs7_unpad(&padded).unwrap();
        assert_eq!(unpadded, data);
    }

    #[test]
    fn test_symmetric_encrypt_decrypt() {
        let provider = CryptoProvider::new(SecurityPolicy::Basic256Sha256);
        let key = vec![0u8; 32];
        let iv = vec![0u8; 16];
        let plaintext = b"Hello, OPC UA!";

        let encrypted = provider.symmetric_encrypt(plaintext, &key, &iv).unwrap();
        let decrypted = provider.symmetric_decrypt(&encrypted.ciphertext, &key, &iv).unwrap();

        assert_eq!(decrypted.plaintext, plaintext);
    }

    #[test]
    fn test_none_policy_passthrough() {
        let provider = CryptoProvider::new(SecurityPolicy::None);
        let data = b"test data";
        let key = vec![0u8; 32];
        let iv = vec![0u8; 16];

        let encrypted = provider.symmetric_encrypt(data, &key, &iv).unwrap();
        assert_eq!(encrypted.ciphertext, data);

        let decrypted = provider.symmetric_decrypt(data, &key, &iv).unwrap();
        assert_eq!(decrypted.plaintext, data);
    }

    #[test]
    fn test_hmac_sign_verify() {
        let provider = CryptoProvider::new(SecurityPolicy::Basic256Sha256);
        let key = vec![0xAB; 32];
        let data = b"Message to sign";

        let signature = provider.hmac_sign(data, &key).unwrap();
        let valid = provider.hmac_verify(data, &key, &signature.signature).unwrap();

        assert!(valid);
    }

    #[test]
    fn test_key_derivation() {
        let provider = CryptoProvider::new(SecurityPolicy::Basic256Sha256);
        let secret = vec![0x42; 32];
        let seed = vec![0x13; 32];

        let keys = provider.derive_keys(&secret, &seed, 32, 32, 16).unwrap();

        assert_eq!(keys.signing_key.len(), 32);
        assert_eq!(keys.encrypting_key.len(), 32);
        assert_eq!(keys.iv.len(), 16);
    }

    #[test]
    fn test_nonce_generation() {
        let provider = CryptoProvider::new(SecurityPolicy::Basic256Sha256);
        let nonce = provider.generate_nonce();

        assert_eq!(nonce.len(), 32);
    }

    #[test]
    fn test_constant_time_compare() {
        assert!(constant_time_compare(b"test", b"test"));
        assert!(!constant_time_compare(b"test", b"Test"));
        assert!(!constant_time_compare(b"test", b"testing"));
    }
}
