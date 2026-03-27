//! Internal provider-based security runtime building blocks.
//!
//! These types keep the public `SecurityManager` surface stable while splitting
//! the concrete responsibilities into trust, policy, identity, and secure
//! channel runtime components.

use std::sync::Arc;

use dashmap::DashMap;
use tracing::{debug, info};

use super::certificate::{Certificate, CertificateManager, ValidationResult};
use super::crypto::{CryptoProvider, CryptoProviderConfig};
use super::manager::{SecurityContext, SecurityError, SecurityManagerConfig, SecurityResult};
use super::policy::{SecurityPolicyConfig, SecurityPolicyProvider};
use super::user_auth::{
    AuthenticationResult, UserAccount, UserAuthenticator, UserCredentials, UserTokenPolicy,
};
use crate::config::{MessageSecurityMode, SecurityPolicy};

pub(crate) trait TrustStorePort: Send + Sync {
    fn initialize(&self) -> SecurityResult<()>;
    fn certificate_manager(&self) -> &Arc<CertificateManager>;
    fn server_certificate(&self) -> Option<Certificate>;
    fn validate_client_certificate(&self, certificate: &Certificate) -> ValidationResult;
    fn trust_certificate(&self, certificate: Certificate) -> SecurityResult<()>;
}

pub(crate) trait SecurityPolicyPort: Send + Sync {
    fn enabled_policies(&self) -> &[SecurityPolicy];
    fn is_enabled(&self, policy: SecurityPolicy) -> bool;
    fn get_config(&self, policy: SecurityPolicy) -> Option<&SecurityPolicyConfig>;
    fn validate_security_mode(
        &self,
        policy: SecurityPolicy,
        mode: MessageSecurityMode,
    ) -> SecurityResult<()>;
}

pub(crate) trait IdentityProvider: Send + Sync {
    fn authenticator(&self) -> &UserAuthenticator;
    fn authenticate(&self, credentials: &UserCredentials) -> AuthenticationResult;
    fn add_user(&self, account: UserAccount);
    fn token_policies(&self) -> Vec<UserTokenPolicy>;
}

pub(crate) trait RoleMapper: Send + Sync {
    fn map_roles(&self, roles: Vec<String>) -> Vec<String>;
}

pub(crate) trait SecurityAuditSink: Send + Sync {
    fn on_initialized(&self, _policies: &[SecurityPolicy]) {}
    fn on_certificate_validated(&self, _thumbprint: Option<&str>, _is_valid: bool) {}
    fn on_authentication(&self, _token_type: &str, _success: bool) {}
    fn on_secure_channel_created(&self, _context: &SecurityContext) {}
    fn on_secure_channel_renewed(&self, _context: &SecurityContext) {}
    fn on_secure_channel_closed(&self, _channel_id: u32) {}
    fn on_secure_channel_cleanup(&self, _count: usize) {}
}

pub(crate) struct CertificateTrustProvider {
    certificate_manager: Arc<CertificateManager>,
}

impl CertificateTrustProvider {
    pub(crate) fn new(config: &SecurityManagerConfig) -> Self {
        Self {
            certificate_manager: Arc::new(CertificateManager::new(
                config.certificate_config.clone(),
            )),
        }
    }
}

impl TrustStorePort for CertificateTrustProvider {
    fn initialize(&self) -> SecurityResult<()> {
        self.certificate_manager.initialize()?;
        Ok(())
    }

    fn certificate_manager(&self) -> &Arc<CertificateManager> {
        &self.certificate_manager
    }

    fn server_certificate(&self) -> Option<Certificate> {
        self.certificate_manager.own_certificate()
    }

    fn validate_client_certificate(&self, certificate: &Certificate) -> ValidationResult {
        self.certificate_manager.validate_certificate(certificate)
    }

    fn trust_certificate(&self, certificate: Certificate) -> SecurityResult<()> {
        self.certificate_manager.trust_certificate(certificate)?;
        Ok(())
    }
}

pub(crate) struct PolicyRuntimeProvider {
    provider: SecurityPolicyProvider,
    reject_deprecated_policies: bool,
}

impl PolicyRuntimeProvider {
    pub(crate) fn new(config: &SecurityManagerConfig) -> Self {
        let mut provider = SecurityPolicyProvider::new();
        for policy in &config.enabled_policies {
            provider.enable_policy(*policy);
        }

        Self {
            provider,
            reject_deprecated_policies: config.reject_deprecated_policies,
        }
    }
}

impl SecurityPolicyPort for PolicyRuntimeProvider {
    fn enabled_policies(&self) -> &[SecurityPolicy] {
        self.provider.enabled_policies()
    }

    fn is_enabled(&self, policy: SecurityPolicy) -> bool {
        self.provider.is_enabled(policy)
    }

    fn get_config(&self, policy: SecurityPolicy) -> Option<&SecurityPolicyConfig> {
        self.provider.get_config(policy)
    }

    fn validate_security_mode(
        &self,
        policy: SecurityPolicy,
        mode: MessageSecurityMode,
    ) -> SecurityResult<()> {
        if !self.is_enabled(policy) {
            return Err(SecurityError::PolicyNotSupported(policy));
        }

        if !self.provider.validate_mode_for_policy(policy, mode) {
            return Err(SecurityError::Configuration(format!(
                "Security mode {:?} not valid for policy {:?}",
                mode, policy
            )));
        }

        if self.reject_deprecated_policies {
            if let Some(config) = self.get_config(policy) {
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
}

pub(crate) struct IdentityRuntimeProvider {
    authenticator: UserAuthenticator,
}

impl IdentityRuntimeProvider {
    pub(crate) fn new(
        config: &SecurityManagerConfig,
        certificate_manager: Arc<CertificateManager>,
    ) -> Self {
        Self {
            authenticator: UserAuthenticator::with_certificate_manager(
                config.user_auth_config.clone(),
                certificate_manager,
            ),
        }
    }
}

impl IdentityProvider for IdentityRuntimeProvider {
    fn authenticator(&self) -> &UserAuthenticator {
        &self.authenticator
    }

    fn authenticate(&self, credentials: &UserCredentials) -> AuthenticationResult {
        self.authenticator.authenticate(credentials)
    }

    fn add_user(&self, account: UserAccount) {
        self.authenticator.add_user(account);
    }

    fn token_policies(&self) -> Vec<UserTokenPolicy> {
        self.authenticator.token_policies()
    }
}

pub(crate) struct StaticRoleMapper;

impl RoleMapper for StaticRoleMapper {
    fn map_roles(&self, mut roles: Vec<String>) -> Vec<String> {
        roles.sort();
        roles.dedup();
        roles
    }
}

pub(crate) struct NoopSecurityAuditSink;

impl SecurityAuditSink for NoopSecurityAuditSink {}

pub(crate) struct ChannelSecurityRuntime {
    crypto_config: CryptoProviderConfig,
    secure_channel_lifetime_ms: u64,
    max_secure_channels: usize,
    secure_channels: DashMap<u32, SecurityContext>,
    next_channel_id: std::sync::atomic::AtomicU32,
    next_token_id: std::sync::atomic::AtomicU32,
}

impl ChannelSecurityRuntime {
    pub(crate) fn new(config: &SecurityManagerConfig) -> Self {
        Self {
            crypto_config: config.crypto_config.clone(),
            secure_channel_lifetime_ms: config.secure_channel_lifetime_ms,
            max_secure_channels: config.max_secure_channels,
            secure_channels: DashMap::new(),
            next_channel_id: std::sync::atomic::AtomicU32::new(1),
            next_token_id: std::sync::atomic::AtomicU32::new(1),
        }
    }

    pub(crate) fn create_secure_channel(
        &self,
        policy: SecurityPolicy,
        mode: MessageSecurityMode,
        client_certificate: Option<Certificate>,
        client_nonce: &[u8],
        policy_provider: &dyn SecurityPolicyPort,
        trust_provider: &dyn TrustStorePort,
        audit_sink: &dyn SecurityAuditSink,
    ) -> SecurityResult<SecurityContext> {
        policy_provider.validate_security_mode(policy, mode)?;

        if self.secure_channels.len() >= self.max_secure_channels {
            return Err(SecurityError::SecureChannel(
                "Maximum secure channels reached".to_string(),
            ));
        }

        let policy_config = policy_provider
            .get_config(policy)
            .ok_or(SecurityError::PolicyNotSupported(policy))?
            .clone();

        if mode != MessageSecurityMode::None {
            if let Some(ref cert) = client_certificate {
                let validation = trust_provider.validate_client_certificate(cert);
                audit_sink.on_certificate_validated(Some(&cert.thumbprint), validation.is_valid);
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

        let channel_id = self
            .next_channel_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let token_id = self
            .next_token_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let key_material = self.derive_key_material(mode, policy, &policy_config, client_nonce)?;
        let now = chrono::Utc::now();
        let lifetime = chrono::Duration::milliseconds(self.secure_channel_lifetime_ms as i64);

        let context = SecurityContext {
            policy,
            policy_config,
            security_mode: mode,
            client_certificate,
            server_certificate: trust_provider.server_certificate(),
            key_material,
            secure_channel_id: channel_id,
            token_id,
            created_at: now,
            expires_at: now + lifetime,
        };

        self.secure_channels.insert(channel_id, context.clone());
        audit_sink.on_secure_channel_created(&context);

        info!(
            channel_id,
            policy = ?policy,
            mode = ?mode,
            "Secure channel created"
        );

        Ok(context)
    }

    pub(crate) fn get_secure_channel(&self, channel_id: u32) -> Option<SecurityContext> {
        self.secure_channels
            .get(&channel_id)
            .map(|entry| entry.clone())
    }

    pub(crate) fn renew_secure_channel(
        &self,
        channel_id: u32,
        audit_sink: &dyn SecurityAuditSink,
    ) -> SecurityResult<SecurityContext> {
        let mut context = self.secure_channels.get_mut(&channel_id).ok_or_else(|| {
            SecurityError::SecureChannel(format!("Secure channel {} not found", channel_id))
        })?;

        let new_token_id = self
            .next_token_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let now = chrono::Utc::now();
        let lifetime = chrono::Duration::milliseconds(self.secure_channel_lifetime_ms as i64);

        context.token_id = new_token_id;
        context.expires_at = now + lifetime;

        if context.requires_signing() || context.requires_encryption() {
            let crypto = self.crypto_provider(context.policy);
            let nonce = crypto.generate_nonce();
            context.key_material = Some(crypto.derive_keys(
                &nonce,
                &nonce,
                context.policy_config.derived_signature_key_length as usize,
                context.policy_config.symmetric_key_length as usize,
                context.policy_config.symmetric_block_size as usize,
            )?);
        }

        let renewed = context.clone();
        audit_sink.on_secure_channel_renewed(&renewed);
        info!(channel_id, new_token_id, "Secure channel renewed");

        Ok(renewed)
    }

    pub(crate) fn close_secure_channel(
        &self,
        channel_id: u32,
        audit_sink: &dyn SecurityAuditSink,
    ) -> bool {
        if self.secure_channels.remove(&channel_id).is_some() {
            audit_sink.on_secure_channel_closed(channel_id);
            info!(channel_id, "Secure channel closed");
            true
        } else {
            false
        }
    }

    pub(crate) fn cleanup_expired_channels(&self, audit_sink: &dyn SecurityAuditSink) -> usize {
        let expired: Vec<u32> = self
            .secure_channels
            .iter()
            .filter(|entry| entry.value().is_expired())
            .map(|entry| *entry.key())
            .collect();

        let count = expired.len();
        for channel_id in expired {
            self.secure_channels.remove(&channel_id);
            audit_sink.on_secure_channel_closed(channel_id);
        }

        if count > 0 {
            audit_sink.on_secure_channel_cleanup(count);
            debug!(count, "Cleaned up expired secure channels");
        }

        count
    }

    pub(crate) fn secure_channel_count(&self) -> usize {
        self.secure_channels.len()
    }

    pub(crate) fn crypto_provider(&self, policy: SecurityPolicy) -> CryptoProvider {
        CryptoProvider::with_config(policy, self.crypto_config.clone())
    }

    pub(crate) fn sign_message(&self, channel_id: u32, message: &[u8]) -> SecurityResult<Vec<u8>> {
        let context = self.require_channel(channel_id)?;
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

    pub(crate) fn verify_signature(
        &self,
        channel_id: u32,
        message: &[u8],
        signature: &[u8],
    ) -> SecurityResult<bool> {
        let context = self.require_channel(channel_id)?;
        if !context.requires_signing() {
            return Ok(true);
        }

        let key_material = context
            .key_material
            .as_ref()
            .ok_or_else(|| SecurityError::SecureChannel("No key material".to_string()))?;

        let crypto = self.crypto_provider(context.policy);
        crypto
            .hmac_verify(message, &key_material.signing_key, signature)
            .map_err(Into::into)
    }

    pub(crate) fn encrypt_message(
        &self,
        channel_id: u32,
        plaintext: &[u8],
    ) -> SecurityResult<Vec<u8>> {
        let context = self.require_channel(channel_id)?;
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

    pub(crate) fn decrypt_message(
        &self,
        channel_id: u32,
        ciphertext: &[u8],
    ) -> SecurityResult<Vec<u8>> {
        let context = self.require_channel(channel_id)?;
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

    fn derive_key_material(
        &self,
        mode: MessageSecurityMode,
        policy: SecurityPolicy,
        policy_config: &SecurityPolicyConfig,
        client_nonce: &[u8],
    ) -> SecurityResult<Option<super::crypto::KeyMaterial>> {
        if mode == MessageSecurityMode::None {
            return Ok(None);
        }

        let crypto = self.crypto_provider(policy);
        let server_nonce = crypto.generate_nonce();
        let seed: Vec<u8> = [client_nonce, &server_nonce].concat();
        let key_material = crypto.derive_keys(
            &seed,
            &seed,
            policy_config.derived_signature_key_length as usize,
            policy_config.symmetric_key_length as usize,
            policy_config.symmetric_block_size as usize,
        )?;

        Ok(Some(key_material))
    }

    fn require_channel(&self, channel_id: u32) -> SecurityResult<SecurityContext> {
        self.get_secure_channel(channel_id).ok_or_else(|| {
            SecurityError::SecureChannel(format!("Secure channel {} not found", channel_id))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_role_mapper_sorts_and_deduplicates() {
        let mapper = StaticRoleMapper;
        let roles = mapper.map_roles(vec![
            "user".to_string(),
            "admin".to_string(),
            "user".to_string(),
        ]);

        assert_eq!(roles, vec!["admin".to_string(), "user".to_string()]);
    }
}
