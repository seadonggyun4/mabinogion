use std::sync::Arc;

use super::IdentityProvider;
use crate::security::certificate::CertificateManager;
use crate::security::manager::SecurityManagerConfig;
use crate::security::user_auth::{
    AuthenticationResult, UserAccount, UserAuthenticator, UserCredentials, UserTokenPolicy,
};

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
