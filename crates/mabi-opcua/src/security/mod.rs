//! OPC UA Security Module.
//!
//! This module provides comprehensive security support for OPC UA including:
//!
//! - **Security Policies**: None, Basic128Rsa15, Basic256, Basic256Sha256, etc.
//! - **Message Security Modes**: None, Sign, SignAndEncrypt
//! - **Certificate Management**: X.509 certificate handling
//! - **Cryptographic Operations**: RSA, AES, SHA-256, HMAC
//! - **User Authentication**: Anonymous, Username/Password, Certificate
//!
//! ## Architecture
//!
//! ```text
//! SecurityManager
//!     ├── CertificateManager
//!     │       ├── CertificateStore (trusted, rejected, issuer)
//!     │       ├── CertificateValidator
//!     │       └── CertificateGenerator (self-signed)
//!     ├── CryptoProvider
//!     │       ├── SymmetricCrypto (AES)
//!     │       ├── AsymmetricCrypto (RSA)
//!     │       └── HashProvider (SHA-256)
//!     └── UserAuthenticator
//!             ├── AnonymousAuth
//!             ├── UserPasswordAuth
//!             └── CertificateAuth
//! ```
//!
//! ## Security Policies
//!
//! | Policy | Symmetric | Asymmetric | Hash | Key Length |
//! |--------|-----------|------------|------|------------|
//! | None | - | - | - | - |
//! | Basic128Rsa15 | AES-128-CBC | RSA-PKCS1 | SHA-1 | 1024+ |
//! | Basic256 | AES-256-CBC | RSA-OAEP | SHA-1 | 1024+ |
//! | Basic256Sha256 | AES-256-CBC | RSA-OAEP | SHA-256 | 2048+ |
//! | Aes128Sha256RsaOaep | AES-128-CBC | RSA-OAEP-SHA256 | SHA-256 | 2048+ |
//! | Aes256Sha256RsaPss | AES-256-CBC | RSA-PSS-SHA256 | SHA-256 | 2048+ |

pub mod certificate;
pub mod crypto;
pub mod manager;
pub mod policy;
pub(crate) mod providers;
pub mod user_auth;

pub use certificate::{
    Certificate, CertificateManager, CertificateManagerConfig, CertificateStore,
    CertificateValidator, ValidationResult,
};
pub use crypto::{
    AsymmetricAlgorithm, CryptoProvider, CryptoProviderConfig, DecryptionResult, EncryptionResult,
    HashAlgorithm, SignatureResult, SymmetricAlgorithm,
};
pub use manager::{SecurityContext, SecurityManager, SecurityManagerConfig};
pub use policy::{SecurityPolicyConfig, SecurityPolicyProvider};
pub use user_auth::{
    AuthenticationResult, UserAuthConfig, UserAuthenticator, UserCredentials, UserToken,
};
