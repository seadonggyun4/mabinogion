//! Security Manager - unified security management for OPC UA.
//!
//! Coordinates certificate management, cryptographic operations, and user authentication.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info};

use super::certificate::{
    Certificate, CertificateManager, CertificateManagerConfig, ValidationResult,
};
use super::crypto::{CryptoProvider, CryptoProviderConfig, KeyMaterial};
use super::policy::{SecurityPolicyConfig, SecurityPolicyProvider};
use super::user_auth::{
    AuthenticationResult, UserAccount, UserAuthConfig, UserAuthenticator, UserCredentials,
    UserTokenPolicy,
};
use crate::config::{MessageSecurityMode, SecurityPolicy};

/// Security manager error types.
#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("Certificate error: {0}")]
    Certificate(#[from] super::certificate::CertificateError),

    #[error("Crypto error: {0}")]
    Crypto(#[from] super::crypto::CryptoError),

    #[error("Authentication error: {0}")]
    Auth(#[from] super::user_auth::AuthError),

    #[error("Security policy not supported: {0:?}")]
    PolicyNotSupported(SecurityPolicy),

    #[error("Security mode not supported: {0:?}")]
    ModeNotSupported(MessageSecurityMode),

    #[error("Security configuration error: {0}")]
    Configuration(String),

    #[error("Secure channel error: {0}")]
    SecureChannel(String),
}

/// Result type for security operations.
pub type SecurityResult<T> = Result<T, SecurityError>;

/// Security manager configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityManagerConfig {
    /// Certificate manager configuration.
    pub certificate_config: CertificateManagerConfig,
    /// User authentication configuration.
    pub user_auth_config: UserAuthConfig,
    /// Crypto provider configuration.
    pub crypto_config: CryptoProviderConfig,
    /// Enabled security policies.
    pub enabled_policies: Vec<SecurityPolicy>,
    /// Default security policy.
    pub default_policy: SecurityPolicy,
    /// Reject connections with deprecated policies.
    pub reject_deprecated_policies: bool,
    /// Secure channel lifetime in milliseconds.
    pub secure_channel_lifetime_ms: u64,
    /// Maximum secure channels.
    pub max_secure_channels: usize,
}

impl Default for SecurityManagerConfig {
    fn default() -> Self {
        Self {
            certificate_config: CertificateManagerConfig::default(),
            user_auth_config: UserAuthConfig::default(),
            crypto_config: CryptoProviderConfig::default(),
            enabled_policies: vec![SecurityPolicy::None, SecurityPolicy::Basic256Sha256],
            default_policy: SecurityPolicy::None,
            reject_deprecated_policies: false,
            secure_channel_lifetime_ms: 3_600_000, // 1 hour
            max_secure_channels: 1000,
        }
    }
}

/// Security context for a secure channel or session.
#[derive(Debug, Clone)]
pub struct SecurityContext {
    /// Security policy.
    pub policy: SecurityPolicy,
    /// Policy configuration.
    pub policy_config: SecurityPolicyConfig,
    /// Message security mode.
    pub security_mode: MessageSecurityMode,
    /// Client certificate (if applicable).
    pub client_certificate: Option<Certificate>,
    /// Server certificate.
    pub server_certificate: Option<Certificate>,
    /// Derived key material for symmetric operations.
    pub key_material: Option<KeyMaterial>,
    /// Secure channel ID.
    pub secure_channel_id: u32,
    /// Token ID for the secure channel.
    pub token_id: u32,
    /// Channel creation time.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Channel expiration time.
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl SecurityContext {
    /// Create a new security context with no security.
    pub fn none() -> Self {
        let now = chrono::Utc::now();
        Self {
            policy: SecurityPolicy::None,
            policy_config: SecurityPolicyConfig::none(),
            security_mode: MessageSecurityMode::None,
            client_certificate: None,
            server_certificate: None,
            key_material: None,
            secure_channel_id: 0,
            token_id: 0,
            created_at: now,
            expires_at: now + chrono::Duration::hours(1),
        }
    }

    /// Check if the context requires signing.
    pub fn requires_signing(&self) -> bool {
        matches!(
            self.security_mode,
            MessageSecurityMode::Sign | MessageSecurityMode::SignAndEncrypt
        )
    }

    /// Check if the context requires encryption.
    pub fn requires_encryption(&self) -> bool {
        matches!(self.security_mode, MessageSecurityMode::SignAndEncrypt)
    }

    /// Check if the secure channel is expired.
    pub fn is_expired(&self) -> bool {
        chrono::Utc::now() > self.expires_at
    }

    /// Get remaining lifetime in milliseconds.
    pub fn remaining_lifetime_ms(&self) -> i64 {
        (self.expires_at - chrono::Utc::now())
            .num_milliseconds()
            .max(0)
    }
}

/// Security manager for OPC UA server.
///
/// Provides unified management of:
/// - Certificate management (own cert, trusted certs, validation)
/// - Cryptographic operations (encryption, signing, hashing)
/// - User authentication (anonymous, username/password, certificate)
/// - Secure channel management
pub struct SecurityManager {
    config: SecurityManagerConfig,
    certificate_manager: Arc<CertificateManager>,
    policy_provider: SecurityPolicyProvider,
    authenticator: UserAuthenticator,
    secure_channels: dashmap::DashMap<u32, SecurityContext>,
    next_channel_id: std::sync::atomic::AtomicU32,
    next_token_id: std::sync::atomic::AtomicU32,
}

impl SecurityManager {
    /// Create a new security manager.
    pub fn new(config: SecurityManagerConfig) -> Self {
        let certificate_manager =
            Arc::new(CertificateManager::new(config.certificate_config.clone()));

        let mut policy_provider = SecurityPolicyProvider::new();
        for policy in &config.enabled_policies {
            policy_provider.enable_policy(*policy);
        }

        let authenticator = UserAuthenticator::with_certificate_manager(
            config.user_auth_config.clone(),
            certificate_manager.clone(),
        );

        Self {
            config,
            certificate_manager,
            policy_provider,
            authenticator,
            secure_channels: dashmap::DashMap::new(),
            next_channel_id: std::sync::atomic::AtomicU32::new(1),
            next_token_id: std::sync::atomic::AtomicU32::new(1),
        }
    }

    /// Initialize the security manager.
    pub fn initialize(&self) -> SecurityResult<()> {
        info!("Initializing security manager");

        // Initialize certificate manager
        self.certificate_manager.initialize()?;

        // Add default admin user if no users configured
        if self.config.user_auth_config.allow_user_password {
            self.authenticator
                .add_user(UserAccount::admin("admin", "admin"));
            debug!("Added default admin user");
        }

        info!(
            policies = ?self.config.enabled_policies,
            "Security manager initialized"
        );

        Ok(())
    }

    // =========================================================================
    // Policy Management
    // =========================================================================

    /// Get enabled security policies.
    pub fn enabled_policies(&self) -> &[SecurityPolicy] {
        self.policy_provider.enabled_policies()
    }

    /// Check if a policy is enabled.
    pub fn is_policy_enabled(&self, policy: SecurityPolicy) -> bool {
        self.policy_provider.is_enabled(policy)
    }

    /// Get policy configuration.
    pub fn get_policy_config(&self, policy: SecurityPolicy) -> Option<&SecurityPolicyConfig> {
        self.policy_provider.get_config(policy)
    }

    /// Validate security mode for policy.
    pub fn validate_security_mode(
        &self,
        policy: SecurityPolicy,
        mode: MessageSecurityMode,
    ) -> SecurityResult<()> {
        if !self.is_policy_enabled(policy) {
            return Err(SecurityError::PolicyNotSupported(policy));
        }

        if !self.policy_provider.validate_mode_for_policy(policy, mode) {
            return Err(SecurityError::Configuration(format!(
                "Security mode {:?} not valid for policy {:?}",
                mode, policy
            )));
        }

        // Check deprecated policy rejection
        if self.config.reject_deprecated_policies {
            if let Some(config) = self.get_policy_config(policy) {
                if config.is_deprecated() {
                    return Err(SecurityError::Configuration(format!(
                        "Policy {:?} is deprecated and rejected",
                        policy
                    )));
                }
            }
        }

        Ok(())
    }

    // =========================================================================
    // Certificate Management
    // =========================================================================

    /// Get the certificate manager.
    pub fn certificate_manager(&self) -> &Arc<CertificateManager> {
        &self.certificate_manager
    }

    /// Get the server's own certificate.
    pub fn server_certificate(&self) -> Option<Certificate> {
        self.certificate_manager.own_certificate()
    }

    /// Validate a client certificate.
    pub fn validate_client_certificate(&self, certificate: &Certificate) -> ValidationResult {
        self.certificate_manager.validate_certificate(certificate)
    }

    /// Trust a client certificate.
    pub fn trust_certificate(&self, certificate: Certificate) -> SecurityResult<()> {
        self.certificate_manager.trust_certificate(certificate)?;
        Ok(())
    }

    // =========================================================================
    // User Authentication
    // =========================================================================

    /// Get the user authenticator.
    pub fn authenticator(&self) -> &UserAuthenticator {
        &self.authenticator
    }

    /// Authenticate user credentials.
    pub fn authenticate(&self, credentials: &UserCredentials) -> AuthenticationResult {
        self.authenticator.authenticate(credentials)
    }

    /// Add a user account.
    pub fn add_user(&self, account: UserAccount) {
        self.authenticator.add_user(account);
    }

    /// Get available user token policies.
    pub fn user_token_policies(&self) -> Vec<UserTokenPolicy> {
        self.authenticator.token_policies()
    }

    // =========================================================================
    // Secure Channel Management
    // =========================================================================

    /// Create a secure channel.
    pub fn create_secure_channel(
        &self,
        policy: SecurityPolicy,
        mode: MessageSecurityMode,
        client_certificate: Option<Certificate>,
        client_nonce: &[u8],
    ) -> SecurityResult<SecurityContext> {
        // Validate policy and mode
        self.validate_security_mode(policy, mode)?;

        // Check channel limit
        if self.secure_channels.len() >= self.config.max_secure_channels {
            return Err(SecurityError::SecureChannel(
                "Maximum secure channels reached".to_string(),
            ));
        }

        // Get policy configuration
        let policy_config = self
            .get_policy_config(policy)
            .ok_or_else(|| SecurityError::PolicyNotSupported(policy))?
            .clone();

        // Validate client certificate if required
        if mode != MessageSecurityMode::None {
            if let Some(ref cert) = client_certificate {
                let validation = self.validate_client_certificate(cert);
                if !validation.is_valid {
                    return Err(SecurityError::Certificate(
                        super::certificate::CertificateError::Invalid(format!(
                            "Client certificate validation failed: {:?}",
                            validation.status
                        )),
                    ));
                }
            }
        }

        // Generate IDs
        let channel_id = self
            .next_channel_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let token_id = self
            .next_token_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        // Derive key material if needed
        let key_material = if mode != MessageSecurityMode::None {
            let crypto = CryptoProvider::with_config(policy, self.config.crypto_config.clone());
            let server_nonce = crypto.generate_nonce();

            // Derive keys
            let seed: Vec<u8> = [client_nonce, &server_nonce].concat();
            Some(crypto.derive_keys(
                &seed,
                &seed,
                policy_config.derived_signature_key_length as usize,
                policy_config.symmetric_key_length as usize,
                policy_config.symmetric_block_size as usize,
            )?)
        } else {
            None
        };

        // Create context
        let now = chrono::Utc::now();
        let lifetime =
            chrono::Duration::milliseconds(self.config.secure_channel_lifetime_ms as i64);

        let context = SecurityContext {
            policy,
            policy_config,
            security_mode: mode,
            client_certificate,
            server_certificate: self.server_certificate(),
            key_material,
            secure_channel_id: channel_id,
            token_id,
            created_at: now,
            expires_at: now + lifetime,
        };

        // Store channel
        self.secure_channels.insert(channel_id, context.clone());

        info!(
            channel_id,
            policy = ?policy,
            mode = ?mode,
            "Secure channel created"
        );

        Ok(context)
    }

    /// Get a secure channel by ID.
    pub fn get_secure_channel(&self, channel_id: u32) -> Option<SecurityContext> {
        self.secure_channels.get(&channel_id).map(|c| c.clone())
    }

    /// Renew a secure channel (create new token).
    pub fn renew_secure_channel(&self, channel_id: u32) -> SecurityResult<SecurityContext> {
        let mut context = self.secure_channels.get_mut(&channel_id).ok_or_else(|| {
            SecurityError::SecureChannel(format!("Secure channel {} not found", channel_id))
        })?;

        // Generate new token
        let new_token_id = self
            .next_token_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        // Update expiration
        let now = chrono::Utc::now();
        let lifetime =
            chrono::Duration::milliseconds(self.config.secure_channel_lifetime_ms as i64);

        context.token_id = new_token_id;
        context.expires_at = now + lifetime;

        // Re-derive keys if security is enabled
        if context.requires_signing() || context.requires_encryption() {
            let crypto =
                CryptoProvider::with_config(context.policy, self.config.crypto_config.clone());
            let nonce = crypto.generate_nonce();

            context.key_material = Some(crypto.derive_keys(
                &nonce,
                &nonce,
                context.policy_config.derived_signature_key_length as usize,
                context.policy_config.symmetric_key_length as usize,
                context.policy_config.symmetric_block_size as usize,
            )?);
        }

        info!(channel_id, new_token_id, "Secure channel renewed");

        Ok(context.clone())
    }

    /// Close a secure channel.
    pub fn close_secure_channel(&self, channel_id: u32) -> bool {
        if self.secure_channels.remove(&channel_id).is_some() {
            info!(channel_id, "Secure channel closed");
            true
        } else {
            false
        }
    }

    /// Cleanup expired secure channels.
    pub fn cleanup_expired_channels(&self) -> usize {
        let expired: Vec<u32> = self
            .secure_channels
            .iter()
            .filter(|e| e.value().is_expired())
            .map(|e| *e.key())
            .collect();

        let count = expired.len();
        for channel_id in expired {
            self.secure_channels.remove(&channel_id);
        }

        if count > 0 {
            debug!(count, "Cleaned up expired secure channels");
        }

        count
    }

    /// Get secure channel count.
    pub fn secure_channel_count(&self) -> usize {
        self.secure_channels.len()
    }

    // =========================================================================
    // Cryptographic Operations
    // =========================================================================

    /// Create a crypto provider for a security policy.
    pub fn crypto_provider(&self, policy: SecurityPolicy) -> CryptoProvider {
        CryptoProvider::with_config(policy, self.config.crypto_config.clone())
    }

    /// Sign a message using the secure channel's key material.
    pub fn sign_message(&self, channel_id: u32, message: &[u8]) -> SecurityResult<Vec<u8>> {
        let context = self.get_secure_channel(channel_id).ok_or_else(|| {
            SecurityError::SecureChannel(format!("Secure channel {} not found", channel_id))
        })?;

        if !context.requires_signing() {
            return Ok(Vec::new());
        }

        let key_material = context
            .key_material
            .as_ref()
            .ok_or_else(|| SecurityError::SecureChannel("No key material".to_string()))?;

        let crypto = self.crypto_provider(context.policy);
        let result = crypto.hmac_sign(message, &key_material.signing_key)?;

        Ok(result.signature)
    }

    /// Verify a message signature.
    pub fn verify_signature(
        &self,
        channel_id: u32,
        message: &[u8],
        signature: &[u8],
    ) -> SecurityResult<bool> {
        let context = self.get_secure_channel(channel_id).ok_or_else(|| {
            SecurityError::SecureChannel(format!("Secure channel {} not found", channel_id))
        })?;

        if !context.requires_signing() {
            return Ok(true);
        }

        let key_material = context
            .key_material
            .as_ref()
            .ok_or_else(|| SecurityError::SecureChannel("No key material".to_string()))?;

        let crypto = self.crypto_provider(context.policy);
        let valid = crypto.hmac_verify(message, &key_material.signing_key, signature)?;

        Ok(valid)
    }

    /// Encrypt a message.
    pub fn encrypt_message(&self, channel_id: u32, plaintext: &[u8]) -> SecurityResult<Vec<u8>> {
        let context = self.get_secure_channel(channel_id).ok_or_else(|| {
            SecurityError::SecureChannel(format!("Secure channel {} not found", channel_id))
        })?;

        if !context.requires_encryption() {
            return Ok(plaintext.to_vec());
        }

        let key_material = context
            .key_material
            .as_ref()
            .ok_or_else(|| SecurityError::SecureChannel("No key material".to_string()))?;

        let crypto = self.crypto_provider(context.policy);
        let result =
            crypto.symmetric_encrypt(plaintext, &key_material.encrypting_key, &key_material.iv)?;

        Ok(result.ciphertext)
    }

    /// Decrypt a message.
    pub fn decrypt_message(&self, channel_id: u32, ciphertext: &[u8]) -> SecurityResult<Vec<u8>> {
        let context = self.get_secure_channel(channel_id).ok_or_else(|| {
            SecurityError::SecureChannel(format!("Secure channel {} not found", channel_id))
        })?;

        if !context.requires_encryption() {
            return Ok(ciphertext.to_vec());
        }

        let key_material = context
            .key_material
            .as_ref()
            .ok_or_else(|| SecurityError::SecureChannel("No key material".to_string()))?;

        let crypto = self.crypto_provider(context.policy);
        let result =
            crypto.symmetric_decrypt(ciphertext, &key_material.encrypting_key, &key_material.iv)?;

        Ok(result.plaintext)
    }
}

impl Default for SecurityManager {
    fn default() -> Self {
        Self::new(SecurityManagerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_manager_creation() {
        let manager = SecurityManager::default();
        manager.initialize().unwrap();

        assert!(manager.is_policy_enabled(SecurityPolicy::None));
        assert!(manager.is_policy_enabled(SecurityPolicy::Basic256Sha256));
    }

    #[test]
    fn test_create_secure_channel_none() {
        let manager = SecurityManager::default();
        manager.initialize().unwrap();

        let context = manager
            .create_secure_channel(SecurityPolicy::None, MessageSecurityMode::None, None, &[])
            .unwrap();

        assert_eq!(context.policy, SecurityPolicy::None);
        assert_eq!(context.security_mode, MessageSecurityMode::None);
        assert!(!context.requires_signing());
        assert!(!context.requires_encryption());
    }

    #[test]
    fn test_create_secure_channel_encrypted() {
        let manager = SecurityManager::default();
        manager.initialize().unwrap();

        let context = manager
            .create_secure_channel(
                SecurityPolicy::Basic256Sha256,
                MessageSecurityMode::SignAndEncrypt,
                None,
                &[0u8; 32],
            )
            .unwrap();

        assert_eq!(context.policy, SecurityPolicy::Basic256Sha256);
        assert!(context.requires_signing());
        assert!(context.requires_encryption());
        assert!(context.key_material.is_some());
    }

    #[test]
    fn test_secure_channel_lifecycle() {
        let manager = SecurityManager::default();
        manager.initialize().unwrap();

        // Create channel
        let context = manager
            .create_secure_channel(SecurityPolicy::None, MessageSecurityMode::None, None, &[])
            .unwrap();

        let channel_id = context.secure_channel_id;
        assert!(manager.get_secure_channel(channel_id).is_some());

        // Close channel
        assert!(manager.close_secure_channel(channel_id));
        assert!(manager.get_secure_channel(channel_id).is_none());
    }

    #[test]
    fn test_authentication() {
        let manager = SecurityManager::default();
        manager.initialize().unwrap();

        // Anonymous auth
        let result = manager.authenticate(&UserCredentials::Anonymous);
        assert!(result.success);

        // Username auth (admin user)
        let result = manager.authenticate(&UserCredentials::user_password("admin", "admin"));
        assert!(result.success);
        assert!(result.roles.contains(&"admin".to_string()));

        // Invalid credentials
        let result = manager.authenticate(&UserCredentials::user_password("admin", "wrong"));
        assert!(!result.success);
    }

    #[test]
    fn test_message_sign_verify() {
        let manager = SecurityManager::default();
        manager.initialize().unwrap();

        let context = manager
            .create_secure_channel(
                SecurityPolicy::Basic256Sha256,
                MessageSecurityMode::Sign,
                None,
                &[0u8; 32],
            )
            .unwrap();

        let message = b"Test message to sign";
        let signature = manager
            .sign_message(context.secure_channel_id, message)
            .unwrap();

        let valid = manager
            .verify_signature(context.secure_channel_id, message, &signature)
            .unwrap();

        assert!(valid);
    }

    #[test]
    fn test_message_encrypt_decrypt() {
        let manager = SecurityManager::default();
        manager.initialize().unwrap();

        let context = manager
            .create_secure_channel(
                SecurityPolicy::Basic256Sha256,
                MessageSecurityMode::SignAndEncrypt,
                None,
                &[0u8; 32],
            )
            .unwrap();

        let plaintext = b"Secret message to encrypt";
        let ciphertext = manager
            .encrypt_message(context.secure_channel_id, plaintext)
            .unwrap();
        let decrypted = manager
            .decrypt_message(context.secure_channel_id, &ciphertext)
            .unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_policy_validation() {
        let manager = SecurityManager::default();

        // Valid combinations
        assert!(manager
            .validate_security_mode(SecurityPolicy::None, MessageSecurityMode::None)
            .is_ok());
        assert!(manager
            .validate_security_mode(SecurityPolicy::Basic256Sha256, MessageSecurityMode::Sign)
            .is_ok());
        assert!(manager
            .validate_security_mode(
                SecurityPolicy::Basic256Sha256,
                MessageSecurityMode::SignAndEncrypt
            )
            .is_ok());

        // Invalid combinations
        assert!(manager
            .validate_security_mode(SecurityPolicy::None, MessageSecurityMode::Sign)
            .is_err());
        assert!(manager
            .validate_security_mode(SecurityPolicy::Basic256Sha256, MessageSecurityMode::None)
            .is_err());
    }
}
