//! Security Policy definitions and configurations.
//!
//! Defines the cryptographic algorithms and parameters for each security policy.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::config::{SecurityPolicy, MessageSecurityMode};

/// Security policy configuration with all cryptographic parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPolicyConfig {
    /// Policy identifier.
    pub policy: SecurityPolicy,
    /// Policy URI.
    pub uri: String,
    /// Symmetric signature algorithm.
    pub symmetric_signature_algorithm: Option<String>,
    /// Symmetric encryption algorithm.
    pub symmetric_encryption_algorithm: Option<String>,
    /// Asymmetric signature algorithm.
    pub asymmetric_signature_algorithm: Option<String>,
    /// Asymmetric encryption algorithm.
    pub asymmetric_encryption_algorithm: Option<String>,
    /// Asymmetric key wrap algorithm.
    pub asymmetric_key_wrap_algorithm: Option<String>,
    /// Key derivation algorithm.
    pub key_derivation_algorithm: Option<String>,
    /// Minimum asymmetric key length (bits).
    pub min_asymmetric_key_length: u32,
    /// Maximum asymmetric key length (bits).
    pub max_asymmetric_key_length: u32,
    /// Derived signature key length (bytes).
    pub derived_signature_key_length: u32,
    /// Symmetric key length (bytes).
    pub symmetric_key_length: u32,
    /// Symmetric block size (bytes).
    pub symmetric_block_size: u32,
    /// Certificate signature algorithm.
    pub certificate_signature_algorithm: Option<String>,
    /// Secure channel nonce length (bytes).
    pub secure_channel_nonce_length: u32,
    /// Deprecated flag.
    pub deprecated: bool,
}

impl Default for SecurityPolicyConfig {
    fn default() -> Self {
        Self::none()
    }
}

impl SecurityPolicyConfig {
    /// Create None security policy (no security).
    pub fn none() -> Self {
        Self {
            policy: SecurityPolicy::None,
            uri: SecurityPolicy::None.uri().to_string(),
            symmetric_signature_algorithm: None,
            symmetric_encryption_algorithm: None,
            asymmetric_signature_algorithm: None,
            asymmetric_encryption_algorithm: None,
            asymmetric_key_wrap_algorithm: None,
            key_derivation_algorithm: None,
            min_asymmetric_key_length: 0,
            max_asymmetric_key_length: 0,
            derived_signature_key_length: 0,
            symmetric_key_length: 0,
            symmetric_block_size: 0,
            certificate_signature_algorithm: None,
            secure_channel_nonce_length: 0,
            deprecated: false,
        }
    }

    /// Create Basic128Rsa15 security policy (deprecated).
    pub fn basic128_rsa15() -> Self {
        Self {
            policy: SecurityPolicy::Basic128Rsa15,
            uri: SecurityPolicy::Basic128Rsa15.uri().to_string(),
            symmetric_signature_algorithm: Some("http://www.w3.org/2000/09/xmldsig#hmac-sha1".to_string()),
            symmetric_encryption_algorithm: Some("http://www.w3.org/2001/04/xmlenc#aes128-cbc".to_string()),
            asymmetric_signature_algorithm: Some("http://www.w3.org/2000/09/xmldsig#rsa-sha1".to_string()),
            asymmetric_encryption_algorithm: Some("http://www.w3.org/2001/04/xmlenc#rsa-1_5".to_string()),
            asymmetric_key_wrap_algorithm: Some("http://www.w3.org/2001/04/xmlenc#rsa-1_5".to_string()),
            key_derivation_algorithm: Some("http://docs.oasis-open.org/ws-sx/ws-secureconversation/200512/dk/p_sha1".to_string()),
            min_asymmetric_key_length: 1024,
            max_asymmetric_key_length: 2048,
            derived_signature_key_length: 16,
            symmetric_key_length: 16,
            symmetric_block_size: 16,
            certificate_signature_algorithm: Some("http://www.w3.org/2000/09/xmldsig#rsa-sha1".to_string()),
            secure_channel_nonce_length: 16,
            deprecated: true,
        }
    }

    /// Create Basic256 security policy (deprecated).
    pub fn basic256() -> Self {
        Self {
            policy: SecurityPolicy::Basic256,
            uri: SecurityPolicy::Basic256.uri().to_string(),
            symmetric_signature_algorithm: Some("http://www.w3.org/2000/09/xmldsig#hmac-sha1".to_string()),
            symmetric_encryption_algorithm: Some("http://www.w3.org/2001/04/xmlenc#aes256-cbc".to_string()),
            asymmetric_signature_algorithm: Some("http://www.w3.org/2000/09/xmldsig#rsa-sha1".to_string()),
            asymmetric_encryption_algorithm: Some("http://www.w3.org/2001/04/xmlenc#rsa-oaep".to_string()),
            asymmetric_key_wrap_algorithm: Some("http://www.w3.org/2001/04/xmlenc#rsa-oaep".to_string()),
            key_derivation_algorithm: Some("http://docs.oasis-open.org/ws-sx/ws-secureconversation/200512/dk/p_sha1".to_string()),
            min_asymmetric_key_length: 1024,
            max_asymmetric_key_length: 2048,
            derived_signature_key_length: 24,
            symmetric_key_length: 32,
            symmetric_block_size: 16,
            certificate_signature_algorithm: Some("http://www.w3.org/2000/09/xmldsig#rsa-sha1".to_string()),
            secure_channel_nonce_length: 32,
            deprecated: true,
        }
    }

    /// Create Basic256Sha256 security policy (recommended).
    pub fn basic256_sha256() -> Self {
        Self {
            policy: SecurityPolicy::Basic256Sha256,
            uri: SecurityPolicy::Basic256Sha256.uri().to_string(),
            symmetric_signature_algorithm: Some("http://www.w3.org/2000/09/xmldsig#hmac-sha256".to_string()),
            symmetric_encryption_algorithm: Some("http://www.w3.org/2001/04/xmlenc#aes256-cbc".to_string()),
            asymmetric_signature_algorithm: Some("http://www.w3.org/2001/04/xmldsig#rsa-sha256".to_string()),
            asymmetric_encryption_algorithm: Some("http://www.w3.org/2001/04/xmlenc#rsa-oaep".to_string()),
            asymmetric_key_wrap_algorithm: Some("http://www.w3.org/2001/04/xmlenc#rsa-oaep".to_string()),
            key_derivation_algorithm: Some("http://docs.oasis-open.org/ws-sx/ws-secureconversation/200512/dk/p_sha256".to_string()),
            min_asymmetric_key_length: 2048,
            max_asymmetric_key_length: 4096,
            derived_signature_key_length: 32,
            symmetric_key_length: 32,
            symmetric_block_size: 16,
            certificate_signature_algorithm: Some("http://www.w3.org/2001/04/xmldsig-more#rsa-sha256".to_string()),
            secure_channel_nonce_length: 32,
            deprecated: false,
        }
    }

    /// Create Aes128Sha256RsaOaep security policy.
    pub fn aes128_sha256_rsa_oaep() -> Self {
        Self {
            policy: SecurityPolicy::Aes128Sha256RsaOaep,
            uri: SecurityPolicy::Aes128Sha256RsaOaep.uri().to_string(),
            symmetric_signature_algorithm: Some("http://www.w3.org/2000/09/xmldsig#hmac-sha256".to_string()),
            symmetric_encryption_algorithm: Some("http://www.w3.org/2001/04/xmlenc#aes128-cbc".to_string()),
            asymmetric_signature_algorithm: Some("http://www.w3.org/2001/04/xmldsig#rsa-sha256".to_string()),
            asymmetric_encryption_algorithm: Some("http://www.w3.org/2001/04/xmlenc#rsa-oaep".to_string()),
            asymmetric_key_wrap_algorithm: Some("http://www.w3.org/2001/04/xmlenc#rsa-oaep".to_string()),
            key_derivation_algorithm: Some("http://docs.oasis-open.org/ws-sx/ws-secureconversation/200512/dk/p_sha256".to_string()),
            min_asymmetric_key_length: 2048,
            max_asymmetric_key_length: 4096,
            derived_signature_key_length: 32,
            symmetric_key_length: 16,
            symmetric_block_size: 16,
            certificate_signature_algorithm: Some("http://www.w3.org/2001/04/xmldsig-more#rsa-sha256".to_string()),
            secure_channel_nonce_length: 32,
            deprecated: false,
        }
    }

    /// Create Aes256Sha256RsaPss security policy.
    pub fn aes256_sha256_rsa_pss() -> Self {
        Self {
            policy: SecurityPolicy::Aes256Sha256RsaPss,
            uri: SecurityPolicy::Aes256Sha256RsaPss.uri().to_string(),
            symmetric_signature_algorithm: Some("http://www.w3.org/2000/09/xmldsig#hmac-sha256".to_string()),
            symmetric_encryption_algorithm: Some("http://www.w3.org/2001/04/xmlenc#aes256-cbc".to_string()),
            asymmetric_signature_algorithm: Some("http://www.w3.org/2007/05/xmldsig-more#sha256-rsa-MGF1".to_string()),
            asymmetric_encryption_algorithm: Some("http://opcfoundation.org/UA/security/rsa-oaep-sha2-256".to_string()),
            asymmetric_key_wrap_algorithm: Some("http://opcfoundation.org/UA/security/rsa-oaep-sha2-256".to_string()),
            key_derivation_algorithm: Some("http://docs.oasis-open.org/ws-sx/ws-secureconversation/200512/dk/p_sha256".to_string()),
            min_asymmetric_key_length: 2048,
            max_asymmetric_key_length: 4096,
            derived_signature_key_length: 32,
            symmetric_key_length: 32,
            symmetric_block_size: 16,
            certificate_signature_algorithm: Some("http://www.w3.org/2007/05/xmldsig-more#sha256-rsa-MGF1".to_string()),
            secure_channel_nonce_length: 32,
            deprecated: false,
        }
    }

    /// Get configuration for a security policy.
    pub fn for_policy(policy: SecurityPolicy) -> Self {
        match policy {
            SecurityPolicy::None => Self::none(),
            SecurityPolicy::Basic128Rsa15 => Self::basic128_rsa15(),
            SecurityPolicy::Basic256 => Self::basic256(),
            SecurityPolicy::Basic256Sha256 => Self::basic256_sha256(),
            SecurityPolicy::Aes128Sha256RsaOaep => Self::aes128_sha256_rsa_oaep(),
            SecurityPolicy::Aes256Sha256RsaPss => Self::aes256_sha256_rsa_pss(),
        }
    }

    /// Check if policy requires encryption.
    pub fn requires_encryption(&self) -> bool {
        self.symmetric_encryption_algorithm.is_some()
    }

    /// Check if policy requires signing.
    pub fn requires_signing(&self) -> bool {
        self.asymmetric_signature_algorithm.is_some()
    }

    /// Check if the policy is deprecated.
    pub fn is_deprecated(&self) -> bool {
        self.deprecated
    }
}

/// Security policy provider that manages available policies.
#[derive(Debug)]
pub struct SecurityPolicyProvider {
    policies: HashMap<SecurityPolicy, SecurityPolicyConfig>,
    enabled_policies: Vec<SecurityPolicy>,
}

impl SecurityPolicyProvider {
    /// Create a new provider with default policies.
    pub fn new() -> Self {
        let mut policies = HashMap::new();
        policies.insert(SecurityPolicy::None, SecurityPolicyConfig::none());
        policies.insert(SecurityPolicy::Basic128Rsa15, SecurityPolicyConfig::basic128_rsa15());
        policies.insert(SecurityPolicy::Basic256, SecurityPolicyConfig::basic256());
        policies.insert(SecurityPolicy::Basic256Sha256, SecurityPolicyConfig::basic256_sha256());
        policies.insert(SecurityPolicy::Aes128Sha256RsaOaep, SecurityPolicyConfig::aes128_sha256_rsa_oaep());
        policies.insert(SecurityPolicy::Aes256Sha256RsaPss, SecurityPolicyConfig::aes256_sha256_rsa_pss());

        // By default, enable None and Basic256Sha256
        let enabled_policies = vec![
            SecurityPolicy::None,
            SecurityPolicy::Basic256Sha256,
        ];

        Self {
            policies,
            enabled_policies,
        }
    }

    /// Create a provider with all policies enabled.
    pub fn all_policies() -> Self {
        let mut provider = Self::new();
        provider.enabled_policies = vec![
            SecurityPolicy::None,
            SecurityPolicy::Basic128Rsa15,
            SecurityPolicy::Basic256,
            SecurityPolicy::Basic256Sha256,
            SecurityPolicy::Aes128Sha256RsaOaep,
            SecurityPolicy::Aes256Sha256RsaPss,
        ];
        provider
    }

    /// Create a provider with recommended policies only (non-deprecated).
    pub fn recommended_policies() -> Self {
        let mut provider = Self::new();
        provider.enabled_policies = vec![
            SecurityPolicy::None,
            SecurityPolicy::Basic256Sha256,
            SecurityPolicy::Aes128Sha256RsaOaep,
            SecurityPolicy::Aes256Sha256RsaPss,
        ];
        provider
    }

    /// Enable a specific policy.
    pub fn enable_policy(&mut self, policy: SecurityPolicy) {
        if !self.enabled_policies.contains(&policy) {
            self.enabled_policies.push(policy);
        }
    }

    /// Disable a specific policy.
    pub fn disable_policy(&mut self, policy: SecurityPolicy) {
        self.enabled_policies.retain(|p| *p != policy);
    }

    /// Get configuration for a policy.
    pub fn get_config(&self, policy: SecurityPolicy) -> Option<&SecurityPolicyConfig> {
        self.policies.get(&policy)
    }

    /// Get all enabled policies.
    pub fn enabled_policies(&self) -> &[SecurityPolicy] {
        &self.enabled_policies
    }

    /// Check if a policy is enabled.
    pub fn is_enabled(&self, policy: SecurityPolicy) -> bool {
        self.enabled_policies.contains(&policy)
    }

    /// Validate that the security mode is compatible with the policy.
    pub fn validate_mode_for_policy(
        &self,
        policy: SecurityPolicy,
        mode: MessageSecurityMode,
    ) -> bool {
        match policy {
            SecurityPolicy::None => mode == MessageSecurityMode::None,
            _ => mode != MessageSecurityMode::None,
        }
    }

    /// Get policy by URI.
    pub fn get_by_uri(&self, uri: &str) -> Option<SecurityPolicy> {
        for (policy, config) in &self.policies {
            if config.uri == uri {
                return Some(*policy);
            }
        }
        None
    }
}

impl Default for SecurityPolicyProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_config_none() {
        let config = SecurityPolicyConfig::none();
        assert_eq!(config.policy, SecurityPolicy::None);
        assert!(!config.requires_encryption());
        assert!(!config.requires_signing());
    }

    #[test]
    fn test_policy_config_basic256_sha256() {
        let config = SecurityPolicyConfig::basic256_sha256();
        assert_eq!(config.policy, SecurityPolicy::Basic256Sha256);
        assert!(config.requires_encryption());
        assert!(config.requires_signing());
        assert_eq!(config.min_asymmetric_key_length, 2048);
        assert!(!config.is_deprecated());
    }

    #[test]
    fn test_deprecated_policies() {
        assert!(SecurityPolicyConfig::basic128_rsa15().is_deprecated());
        assert!(SecurityPolicyConfig::basic256().is_deprecated());
        assert!(!SecurityPolicyConfig::basic256_sha256().is_deprecated());
    }

    #[test]
    fn test_policy_provider() {
        let provider = SecurityPolicyProvider::new();
        assert!(provider.is_enabled(SecurityPolicy::None));
        assert!(provider.is_enabled(SecurityPolicy::Basic256Sha256));
        assert!(!provider.is_enabled(SecurityPolicy::Basic128Rsa15));
    }

    #[test]
    fn test_policy_by_uri() {
        let provider = SecurityPolicyProvider::new();
        let policy = provider.get_by_uri("http://opcfoundation.org/UA/SecurityPolicy#Basic256Sha256");
        assert_eq!(policy, Some(SecurityPolicy::Basic256Sha256));
    }

    #[test]
    fn test_mode_validation() {
        let provider = SecurityPolicyProvider::new();

        // None policy only works with None mode
        assert!(provider.validate_mode_for_policy(SecurityPolicy::None, MessageSecurityMode::None));
        assert!(!provider.validate_mode_for_policy(SecurityPolicy::None, MessageSecurityMode::Sign));

        // Other policies require Sign or SignAndEncrypt
        assert!(!provider.validate_mode_for_policy(SecurityPolicy::Basic256Sha256, MessageSecurityMode::None));
        assert!(provider.validate_mode_for_policy(SecurityPolicy::Basic256Sha256, MessageSecurityMode::Sign));
        assert!(provider.validate_mode_for_policy(SecurityPolicy::Basic256Sha256, MessageSecurityMode::SignAndEncrypt));
    }
}
