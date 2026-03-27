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
use super::policy::SecurityPolicyConfig;
use super::providers::{
    CertificateTrustProvider, ChannelSecurityRuntime, IdentityProvider as _,
    IdentityRuntimeProvider, NoopSecurityAuditSink, PolicyRuntimeProvider, RoleMapper,
    SecurityAuditSink, SecurityPolicyPort as _, StaticRoleMapper, TrustStorePort as _,
};
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
    trust_provider: CertificateTrustProvider,
    policy_provider: PolicyRuntimeProvider,
    identity_provider: IdentityRuntimeProvider,
    channel_runtime: ChannelSecurityRuntime,
    role_mapper: Arc<dyn RoleMapper>,
    audit_sink: Arc<dyn SecurityAuditSink>,
}

impl SecurityManager {
    /// Create a new security manager.
    pub fn new(config: SecurityManagerConfig) -> Self {
        let trust_provider = CertificateTrustProvider::new(&config);
        let policy_provider = PolicyRuntimeProvider::new(&config);
        let identity_provider =
            IdentityRuntimeProvider::new(&config, trust_provider.certificate_manager().clone());
        let channel_runtime = ChannelSecurityRuntime::new(&config);

        Self::with_components(
            config,
            policy_provider,
            trust_provider,
            identity_provider,
            channel_runtime,
            Arc::new(StaticRoleMapper),
            Arc::new(NoopSecurityAuditSink),
        )
    }

    pub(crate) fn with_components(
        config: SecurityManagerConfig,
        policy_provider: PolicyRuntimeProvider,
        trust_provider: CertificateTrustProvider,
        identity_provider: IdentityRuntimeProvider,
        channel_runtime: ChannelSecurityRuntime,
        role_mapper: Arc<dyn RoleMapper>,
        audit_sink: Arc<dyn SecurityAuditSink>,
    ) -> Self {
        Self {
            config,
            trust_provider,
            policy_provider,
            identity_provider,
            channel_runtime,
            role_mapper,
            audit_sink,
        }
    }

    /// Initialize the security manager.
    pub fn initialize(&self) -> SecurityResult<()> {
        info!("Initializing security manager");

        self.trust_provider.initialize()?;

        // Add default admin user if no users configured
        if self.config.user_auth_config.allow_user_password {
            self.identity_provider
                .add_user(UserAccount::admin("admin", "admin"));
            debug!("Added default admin user");
        }

        self.audit_sink
            .on_initialized(self.policy_provider.enabled_policies());

        info!(
            policies = ?self.policy_provider.enabled_policies(),
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
        self.policy_provider.validate_security_mode(policy, mode)
    }

    // =========================================================================
    // Certificate Management
    // =========================================================================

    /// Get the certificate manager.
    pub fn certificate_manager(&self) -> &Arc<CertificateManager> {
        self.trust_provider.certificate_manager()
    }

    /// Get the server's own certificate.
    pub fn server_certificate(&self) -> Option<Certificate> {
        self.trust_provider.server_certificate()
    }

    /// Validate a client certificate.
    pub fn validate_client_certificate(&self, certificate: &Certificate) -> ValidationResult {
        let validation = self.trust_provider.validate_client_certificate(certificate);
        self.audit_sink
            .on_certificate_validated(Some(&certificate.thumbprint), validation.is_valid);
        validation
    }

    /// Trust a client certificate.
    pub fn trust_certificate(&self, certificate: Certificate) -> SecurityResult<()> {
        self.trust_provider.trust_certificate(certificate)
    }

    // =========================================================================
    // User Authentication
    // =========================================================================

    /// Get the user authenticator.
    pub fn authenticator(&self) -> &UserAuthenticator {
        self.identity_provider.authenticator()
    }

    /// Authenticate user credentials.
    pub fn authenticate(&self, credentials: &UserCredentials) -> AuthenticationResult {
        let mut result = self.identity_provider.authenticate(credentials);
        if result.success {
            result.roles = self
                .role_mapper
                .map_roles(std::mem::take(&mut result.roles));
        }
        self.audit_sink
            .on_authentication(credentials.token_type_name(), result.success);
        result
    }

    /// Add a user account.
    pub fn add_user(&self, account: UserAccount) {
        self.identity_provider.add_user(account);
    }

    /// Get available user token policies.
    pub fn user_token_policies(&self) -> Vec<UserTokenPolicy> {
        self.identity_provider.token_policies()
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
        self.channel_runtime.create_secure_channel(
            policy,
            mode,
            client_certificate,
            client_nonce,
            &self.policy_provider,
            &self.trust_provider,
            self.audit_sink.as_ref(),
        )
    }

    /// Get a secure channel by ID.
    pub fn get_secure_channel(&self, channel_id: u32) -> Option<SecurityContext> {
        self.channel_runtime.get_secure_channel(channel_id)
    }

    /// Renew a secure channel (create new token).
    pub fn renew_secure_channel(&self, channel_id: u32) -> SecurityResult<SecurityContext> {
        self.channel_runtime
            .renew_secure_channel(channel_id, self.audit_sink.as_ref())
    }

    /// Close a secure channel.
    pub fn close_secure_channel(&self, channel_id: u32) -> bool {
        self.channel_runtime
            .close_secure_channel(channel_id, self.audit_sink.as_ref())
    }

    /// Cleanup expired secure channels.
    pub fn cleanup_expired_channels(&self) -> usize {
        self.channel_runtime
            .cleanup_expired_channels(self.audit_sink.as_ref())
    }

    /// Get secure channel count.
    pub fn secure_channel_count(&self) -> usize {
        self.channel_runtime.secure_channel_count()
    }

    // =========================================================================
    // Cryptographic Operations
    // =========================================================================

    /// Create a crypto provider for a security policy.
    pub fn crypto_provider(&self, policy: SecurityPolicy) -> CryptoProvider {
        self.channel_runtime.crypto_provider(policy)
    }

    /// Sign a message using the secure channel's key material.
    pub fn sign_message(&self, channel_id: u32, message: &[u8]) -> SecurityResult<Vec<u8>> {
        self.channel_runtime.sign_message(channel_id, message)
    }

    /// Verify a message signature.
    pub fn verify_signature(
        &self,
        channel_id: u32,
        message: &[u8],
        signature: &[u8],
    ) -> SecurityResult<bool> {
        self.channel_runtime
            .verify_signature(channel_id, message, signature)
    }

    /// Encrypt a message.
    pub fn encrypt_message(&self, channel_id: u32, plaintext: &[u8]) -> SecurityResult<Vec<u8>> {
        self.channel_runtime.encrypt_message(channel_id, plaintext)
    }

    /// Decrypt a message.
    pub fn decrypt_message(&self, channel_id: u32, ciphertext: &[u8]) -> SecurityResult<Vec<u8>> {
        self.channel_runtime.decrypt_message(channel_id, ciphertext)
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
