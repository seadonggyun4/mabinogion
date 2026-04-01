use tracing::warn;

use super::SecurityPolicyPort;
use crate::config::{MessageSecurityMode, SecurityPolicy};
use crate::security::manager::{
    DeprecatedPolicyHandling, SecurityError, SecurityManagerConfig, SecurityResult,
};
use crate::security::policy::{SecurityPolicyConfig, SecurityPolicyProvider};

pub(crate) struct PolicyRuntimeProvider {
    provider: SecurityPolicyProvider,
    deprecated_policy_handling: DeprecatedPolicyHandling,
}

impl PolicyRuntimeProvider {
    pub(crate) fn new(config: &SecurityManagerConfig) -> Self {
        let mut provider = SecurityPolicyProvider::new();
        for policy in &config.enabled_policies {
            provider.enable_policy(*policy);
        }

        Self {
            provider,
            deprecated_policy_handling: if config.reject_deprecated_policies {
                DeprecatedPolicyHandling::Reject
            } else {
                config.deprecated_policy_handling
            },
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

        if let Some(config) = self.get_config(policy) {
            if config.is_deprecated() {
                match self.deprecated_policy_handling {
                    DeprecatedPolicyHandling::Allow => {}
                    DeprecatedPolicyHandling::Warn => {
                        warn!(policy = ?policy, "deprecated OPC UA security policy accepted");
                    }
                    DeprecatedPolicyHandling::Reject => {
                        return Err(SecurityError::Configuration(format!(
                            "Policy {:?} is deprecated and rejected",
                            policy
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    fn is_deprecated(&self, policy: SecurityPolicy) -> bool {
        self.get_config(policy)
            .map(SecurityPolicyConfig::is_deprecated)
            .unwrap_or(false)
    }

    fn deprecated_policy_handling(&self) -> DeprecatedPolicyHandling {
        self.deprecated_policy_handling
    }
}
