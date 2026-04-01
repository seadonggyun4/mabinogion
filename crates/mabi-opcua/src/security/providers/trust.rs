use std::sync::Arc;

use super::TrustStorePort;
use crate::security::certificate::{Certificate, CertificateManager, ValidationResult};
use crate::security::manager::{SecurityManagerConfig, SecurityResult};

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

    fn reload_trust_store(&self) -> SecurityResult<()> {
        self.certificate_manager.reload_trust_store()?;
        Ok(())
    }

    fn rotate_server_certificate(
        &self,
        certificate_path: &std::path::Path,
        private_key_path: &std::path::Path,
    ) -> SecurityResult<Certificate> {
        self.certificate_manager
            .rotate_server_certificate(certificate_path, private_key_path)
            .map_err(Into::into)
    }

    fn trusted_certificate_count(&self) -> usize {
        self.certificate_manager.trusted_store().count()
    }

    fn rejected_certificate_count(&self) -> usize {
        self.certificate_manager.rejected_store().count()
    }
}
