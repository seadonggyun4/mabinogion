//! Internal provider-based security runtime building blocks.
//!
//! These types keep the public `SecurityManager` surface stable while splitting
//! the concrete responsibilities into trust, policy, identity, role mapping,
//! audit sinks, and secure-channel runtime components.

mod audit;
mod channel;
mod identity;
mod policy;
mod roles;
mod trust;

use std::sync::Arc;

use super::certificate::{Certificate, CertificateManager, ValidationResult};
use super::manager::{
    DeprecatedPolicyHandling, SecurityAuditStatus, SecurityContext, SecurityResult,
};
use super::policy::SecurityPolicyConfig;
use super::user_auth::{
    AuthenticationResult, UserAccount, UserAuthenticator, UserCredentials, UserTokenPolicy,
};
use crate::config::{MessageSecurityMode, SecurityPolicy};

pub(crate) use audit::build_audit_sink;
pub(crate) use channel::ChannelSecurityRuntime;
pub(crate) use identity::IdentityRuntimeProvider;
pub(crate) use policy::PolicyRuntimeProvider;
pub(crate) use roles::build_role_mapper;
pub(crate) use trust::CertificateTrustProvider;

#[cfg(test)]
use super::manager::SecurityManagerConfig;
#[cfg(test)]
use roles::{ConfigDrivenRoleMapper, StaticRoleMapper};

pub(crate) trait TrustStorePort: Send + Sync {
    fn initialize(&self) -> SecurityResult<()>;
    fn certificate_manager(&self) -> &Arc<CertificateManager>;
    fn server_certificate(&self) -> Option<Certificate>;
    fn validate_client_certificate(&self, certificate: &Certificate) -> ValidationResult;
    fn trust_certificate(&self, certificate: Certificate) -> SecurityResult<()>;
    fn reload_trust_store(&self) -> SecurityResult<()>;
    fn rotate_server_certificate(
        &self,
        certificate_path: &std::path::Path,
        private_key_path: &std::path::Path,
    ) -> SecurityResult<Certificate>;
    fn trusted_certificate_count(&self) -> usize;
    fn rejected_certificate_count(&self) -> usize;
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
    fn is_deprecated(&self, policy: SecurityPolicy) -> bool;
    fn deprecated_policy_handling(&self) -> DeprecatedPolicyHandling;
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
    fn on_deprecated_policy_warning(&self, _policy: SecurityPolicy) {}
    fn on_secure_channel_created(&self, _context: &SecurityContext) {}
    fn on_secure_channel_renewed(&self, _context: &SecurityContext) {}
    fn on_secure_channel_closed(&self, _channel_id: u32) {}
    fn on_secure_channel_cleanup(&self, _count: usize) {}
    fn on_trust_store_reloaded(&self, _trusted: usize, _rejected: usize) {}
    fn on_server_certificate_rotated(&self, _thumbprint: Option<&str>) {}
    fn status(&self) -> SecurityAuditStatus;
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

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

    #[test]
    fn config_role_mapper_applies_rules() {
        let mapper = ConfigDrivenRoleMapper::new(vec![crate::security::manager::RoleMappingRule {
            match_role: "operator".into(),
            add_roles: vec!["viewer".into(), "user".into()],
        }]);
        let roles = mapper.map_roles(vec!["operator".into()]);
        assert_eq!(
            roles,
            vec![
                "operator".to_string(),
                "user".to_string(),
                "viewer".to_string()
            ]
        );
    }

    #[test]
    fn audit_sink_factory_builds_jsonl_sink() {
        let dir = tempdir().unwrap();
        let mut config = SecurityManagerConfig::default();
        config.audit_sink.kind = crate::security::manager::SecurityAuditSinkKind::JsonlFile;
        config.audit_sink.path = Some(dir.path().join("audit.jsonl"));
        let sink = build_audit_sink(&config);
        sink.on_initialized(&[SecurityPolicy::None]);
        let content = std::fs::read_to_string(dir.path().join("audit.jsonl")).unwrap();
        assert!(content.contains("\"event\":\"initialized\""));
    }

    #[test]
    fn deprecated_policy_rejects_when_configured() {
        let mut config = SecurityManagerConfig::default();
        config.enabled_policies = vec![SecurityPolicy::Basic128Rsa15];
        config.deprecated_policy_handling =
            crate::security::manager::DeprecatedPolicyHandling::Reject;
        let provider = PolicyRuntimeProvider::new(&config);
        let result = provider.validate_security_mode(
            SecurityPolicy::Basic128Rsa15,
            MessageSecurityMode::SignAndEncrypt,
        );
        assert!(result.is_err());
    }
}
