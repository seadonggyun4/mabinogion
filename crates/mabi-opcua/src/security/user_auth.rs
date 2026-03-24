//! User authentication for OPC UA.
//!
//! Supports Anonymous, Username/Password, and Certificate-based authentication.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

use super::certificate::{Certificate, CertificateManager};
use super::crypto::CryptoProvider;
use crate::config::SecurityPolicy;

/// Authentication error types.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("User not found: {0}")]
    UserNotFound(String),

    #[error("User disabled: {0}")]
    UserDisabled(String),

    #[error("Password expired")]
    PasswordExpired,

    #[error("Account locked: {0}")]
    AccountLocked(String),

    #[error("Certificate validation failed: {0}")]
    CertificateInvalid(String),

    #[error("Token expired")]
    TokenExpired,

    #[error("Token type not supported: {0}")]
    TokenTypeNotSupported(String),

    #[error("Authentication not allowed: {0}")]
    NotAllowed(String),
}

/// Result type for authentication operations.
pub type AuthResult<T> = Result<T, AuthError>;

/// User credentials.
#[derive(Debug, Clone)]
pub enum UserCredentials {
    /// Anonymous access.
    Anonymous,

    /// Username and password.
    UserPassword { username: String, password: String },

    /// X.509 certificate.
    Certificate { certificate: Certificate },

    /// Issued token (e.g., JWT).
    IssuedToken {
        token_type: String,
        token_data: Vec<u8>,
    },
}

impl UserCredentials {
    /// Create anonymous credentials.
    pub fn anonymous() -> Self {
        Self::Anonymous
    }

    /// Create username/password credentials.
    pub fn user_password(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self::UserPassword {
            username: username.into(),
            password: password.into(),
        }
    }

    /// Create certificate credentials.
    pub fn certificate(certificate: Certificate) -> Self {
        Self::Certificate { certificate }
    }

    /// Create issued token credentials.
    pub fn issued_token(token_type: impl Into<String>, token_data: Vec<u8>) -> Self {
        Self::IssuedToken {
            token_type: token_type.into(),
            token_data,
        }
    }

    /// Get the token type name.
    pub fn token_type_name(&self) -> &str {
        match self {
            Self::Anonymous => "Anonymous",
            Self::UserPassword { .. } => "UserName",
            Self::Certificate { .. } => "X509",
            Self::IssuedToken { token_type, .. } => token_type,
        }
    }
}

/// User token for session authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserToken {
    /// Policy ID.
    pub policy_id: String,
    /// Token type.
    pub token_type: UserTokenType,
    /// Username (for UserName tokens).
    pub username: Option<String>,
    /// Certificate thumbprint (for Certificate tokens).
    pub certificate_thumbprint: Option<String>,
    /// Issued token type.
    pub issued_token_type: Option<String>,
    /// Creation time.
    pub created_at: DateTime<Utc>,
}

/// User token types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserTokenType {
    Anonymous,
    UserName,
    Certificate,
    IssuedToken,
}

impl Default for UserTokenType {
    fn default() -> Self {
        Self::Anonymous
    }
}

/// Authentication result.
#[derive(Debug, Clone)]
pub struct AuthenticationResult {
    /// Whether authentication succeeded.
    pub success: bool,
    /// User token if successful.
    pub user_token: Option<UserToken>,
    /// User identity string.
    pub user_identity: String,
    /// Roles assigned to the user.
    pub roles: Vec<String>,
    /// Error message if failed.
    pub error_message: Option<String>,
}

impl AuthenticationResult {
    /// Create a successful result.
    pub fn success(
        user_token: UserToken,
        user_identity: impl Into<String>,
        roles: Vec<String>,
    ) -> Self {
        Self {
            success: true,
            user_token: Some(user_token),
            user_identity: user_identity.into(),
            roles,
            error_message: None,
        }
    }

    /// Create a failed result.
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            user_token: None,
            user_identity: String::new(),
            roles: Vec::new(),
            error_message: Some(error.into()),
        }
    }
}

/// User account information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAccount {
    /// Username.
    pub username: String,
    /// Password hash.
    pub password_hash: String,
    /// Salt for password hashing.
    pub salt: String,
    /// Is the account enabled.
    pub enabled: bool,
    /// Roles assigned to the user.
    pub roles: Vec<String>,
    /// Last login time.
    pub last_login: Option<DateTime<Utc>>,
    /// Failed login count.
    pub failed_login_count: u32,
    /// Account locked until.
    pub locked_until: Option<DateTime<Utc>>,
    /// Password expiration date.
    pub password_expires: Option<DateTime<Utc>>,
}

impl UserAccount {
    /// Create a new user account.
    pub fn new(username: impl Into<String>, password: &str) -> Self {
        let salt = generate_salt();
        let password_hash = hash_password(password, &salt);

        Self {
            username: username.into(),
            password_hash,
            salt,
            enabled: true,
            roles: vec!["user".to_string()],
            last_login: None,
            failed_login_count: 0,
            locked_until: None,
            password_expires: None,
        }
    }

    /// Create an admin account.
    pub fn admin(username: impl Into<String>, password: &str) -> Self {
        let mut account = Self::new(username, password);
        account.roles = vec!["admin".to_string(), "user".to_string()];
        account
    }

    /// Check if account is locked.
    pub fn is_locked(&self) -> bool {
        if let Some(locked_until) = self.locked_until {
            Utc::now() < locked_until
        } else {
            false
        }
    }

    /// Check if password is expired.
    pub fn is_password_expired(&self) -> bool {
        if let Some(expires) = self.password_expires {
            Utc::now() > expires
        } else {
            false
        }
    }

    /// Verify password.
    pub fn verify_password(&self, password: &str) -> bool {
        let hash = hash_password(password, &self.salt);
        self.password_hash == hash
    }

    /// Record successful login.
    pub fn record_login_success(&mut self) {
        self.last_login = Some(Utc::now());
        self.failed_login_count = 0;
        self.locked_until = None;
    }

    /// Record failed login.
    pub fn record_login_failure(&mut self, lockout_threshold: u32, lockout_duration_minutes: i64) {
        self.failed_login_count += 1;

        if self.failed_login_count >= lockout_threshold {
            self.locked_until =
                Some(Utc::now() + chrono::Duration::minutes(lockout_duration_minutes));
        }
    }
}

/// User authenticator configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAuthConfig {
    /// Allow anonymous access.
    pub allow_anonymous: bool,
    /// Allow username/password authentication.
    pub allow_user_password: bool,
    /// Allow certificate authentication.
    pub allow_certificate: bool,
    /// Allow issued token authentication.
    pub allow_issued_token: bool,
    /// Accepted issued token types.
    pub accepted_token_types: Vec<String>,
    /// Lockout threshold (failed attempts).
    pub lockout_threshold: u32,
    /// Lockout duration in minutes.
    pub lockout_duration_minutes: i64,
    /// Password policy: minimum length.
    pub password_min_length: usize,
    /// Password policy: require uppercase.
    pub password_require_uppercase: bool,
    /// Password policy: require number.
    pub password_require_number: bool,
    /// Password policy: require special character.
    pub password_require_special: bool,
}

impl Default for UserAuthConfig {
    fn default() -> Self {
        Self {
            allow_anonymous: true,
            allow_user_password: true,
            allow_certificate: true,
            allow_issued_token: false,
            accepted_token_types: vec!["http://opcfoundation.org/UA/UserToken#JWT".to_string()],
            lockout_threshold: 5,
            lockout_duration_minutes: 30,
            password_min_length: 8,
            password_require_uppercase: true,
            password_require_number: true,
            password_require_special: false,
        }
    }
}

/// User authenticator for OPC UA sessions.
pub struct UserAuthenticator {
    config: UserAuthConfig,
    users: DashMap<String, UserAccount>,
    certificate_manager: Option<Arc<CertificateManager>>,
    #[allow(dead_code)]
    crypto_provider: CryptoProvider,
}

impl UserAuthenticator {
    /// Create a new authenticator.
    pub fn new(config: UserAuthConfig) -> Self {
        Self {
            config,
            users: DashMap::new(),
            certificate_manager: None,
            crypto_provider: CryptoProvider::new(SecurityPolicy::None),
        }
    }

    /// Create with certificate manager.
    pub fn with_certificate_manager(
        config: UserAuthConfig,
        certificate_manager: Arc<CertificateManager>,
    ) -> Self {
        Self {
            config,
            users: DashMap::new(),
            certificate_manager: Some(certificate_manager),
            crypto_provider: CryptoProvider::new(SecurityPolicy::None),
        }
    }

    /// Add a user account.
    pub fn add_user(&self, account: UserAccount) {
        info!(username = %account.username, "Adding user account");
        self.users.insert(account.username.clone(), account);
    }

    /// Remove a user account.
    pub fn remove_user(&self, username: &str) -> Option<UserAccount> {
        self.users.remove(username).map(|(_, account)| account)
    }

    /// Get a user account.
    pub fn get_user(&self, username: &str) -> Option<UserAccount> {
        self.users.get(username).map(|u| u.clone())
    }

    /// Authenticate user credentials.
    pub fn authenticate(&self, credentials: &UserCredentials) -> AuthenticationResult {
        match credentials {
            UserCredentials::Anonymous => self.authenticate_anonymous(),
            UserCredentials::UserPassword { username, password } => {
                self.authenticate_user_password(username, password)
            }
            UserCredentials::Certificate { certificate } => {
                self.authenticate_certificate(certificate)
            }
            UserCredentials::IssuedToken {
                token_type,
                token_data,
            } => self.authenticate_issued_token(token_type, token_data),
        }
    }

    /// Authenticate anonymous user.
    fn authenticate_anonymous(&self) -> AuthenticationResult {
        if !self.config.allow_anonymous {
            return AuthenticationResult::failure("Anonymous access not allowed");
        }

        let token = UserToken {
            policy_id: "anonymous".to_string(),
            token_type: UserTokenType::Anonymous,
            username: None,
            certificate_thumbprint: None,
            issued_token_type: None,
            created_at: Utc::now(),
        };

        debug!("Anonymous authentication successful");

        AuthenticationResult::success(token, "Anonymous", vec!["anonymous".to_string()])
    }

    /// Authenticate username/password.
    fn authenticate_user_password(&self, username: &str, password: &str) -> AuthenticationResult {
        if !self.config.allow_user_password {
            return AuthenticationResult::failure("Username/password authentication not allowed");
        }

        // Get user account
        let mut user = match self.users.get_mut(username) {
            Some(user) => user,
            None => {
                warn!(username = %username, "User not found");
                return AuthenticationResult::failure("Invalid credentials");
            }
        };

        // Check if account is enabled
        if !user.enabled {
            warn!(username = %username, "User account disabled");
            return AuthenticationResult::failure("Account disabled");
        }

        // Check if account is locked
        if user.is_locked() {
            warn!(username = %username, "User account locked");
            return AuthenticationResult::failure("Account locked");
        }

        // Check if password is expired
        if user.is_password_expired() {
            warn!(username = %username, "Password expired");
            return AuthenticationResult::failure("Password expired");
        }

        // Verify password
        if !user.verify_password(password) {
            user.record_login_failure(
                self.config.lockout_threshold,
                self.config.lockout_duration_minutes,
            );
            warn!(username = %username, failed_count = user.failed_login_count, "Invalid password");
            return AuthenticationResult::failure("Invalid credentials");
        }

        // Success
        user.record_login_success();
        let roles = user.roles.clone();

        let token = UserToken {
            policy_id: "username".to_string(),
            token_type: UserTokenType::UserName,
            username: Some(username.to_string()),
            certificate_thumbprint: None,
            issued_token_type: None,
            created_at: Utc::now(),
        };

        info!(username = %username, "Username/password authentication successful");

        AuthenticationResult::success(token, username, roles)
    }

    /// Authenticate certificate.
    fn authenticate_certificate(&self, certificate: &Certificate) -> AuthenticationResult {
        if !self.config.allow_certificate {
            return AuthenticationResult::failure("Certificate authentication not allowed");
        }

        // Validate certificate
        if let Some(ref cert_manager) = self.certificate_manager {
            let validation = cert_manager.validate_certificate(certificate);

            if !validation.is_valid {
                warn!(
                    thumbprint = %certificate.thumbprint,
                    status = ?validation.status,
                    "Certificate validation failed"
                );
                return AuthenticationResult::failure(format!(
                    "Certificate validation failed: {:?}",
                    validation.status
                ));
            }
        }

        // Extract identity from certificate
        let identity = &certificate.subject;

        let token = UserToken {
            policy_id: "certificate".to_string(),
            token_type: UserTokenType::Certificate,
            username: None,
            certificate_thumbprint: Some(certificate.thumbprint.clone()),
            issued_token_type: None,
            created_at: Utc::now(),
        };

        info!(
            thumbprint = %certificate.thumbprint,
            subject = %identity,
            "Certificate authentication successful"
        );

        // Certificate users typically get 'user' role
        AuthenticationResult::success(token, identity, vec!["user".to_string()])
    }

    /// Authenticate issued token.
    fn authenticate_issued_token(
        &self,
        token_type: &str,
        _token_data: &[u8],
    ) -> AuthenticationResult {
        if !self.config.allow_issued_token {
            return AuthenticationResult::failure("Issued token authentication not allowed");
        }

        if !self
            .config
            .accepted_token_types
            .contains(&token_type.to_string())
        {
            return AuthenticationResult::failure(format!(
                "Token type not supported: {}",
                token_type
            ));
        }

        // In production, validate the token (e.g., verify JWT signature)
        // For simulation, accept any token

        let token = UserToken {
            policy_id: "issued".to_string(),
            token_type: UserTokenType::IssuedToken,
            username: None,
            certificate_thumbprint: None,
            issued_token_type: Some(token_type.to_string()),
            created_at: Utc::now(),
        };

        info!(token_type = %token_type, "Issued token authentication successful");

        AuthenticationResult::success(token, "TokenUser", vec!["user".to_string()])
    }

    /// Validate password policy.
    pub fn validate_password(&self, password: &str) -> Result<(), String> {
        if password.len() < self.config.password_min_length {
            return Err(format!(
                "Password must be at least {} characters",
                self.config.password_min_length
            ));
        }

        if self.config.password_require_uppercase && !password.chars().any(|c| c.is_uppercase()) {
            return Err("Password must contain at least one uppercase letter".to_string());
        }

        if self.config.password_require_number && !password.chars().any(|c| c.is_numeric()) {
            return Err("Password must contain at least one number".to_string());
        }

        if self.config.password_require_special {
            let special_chars = "!@#$%^&*()_+-=[]{}|;':\",./<>?";
            if !password.chars().any(|c| special_chars.contains(c)) {
                return Err("Password must contain at least one special character".to_string());
            }
        }

        Ok(())
    }

    /// Get available user token policies.
    pub fn token_policies(&self) -> Vec<UserTokenPolicy> {
        let mut policies = Vec::new();

        if self.config.allow_anonymous {
            policies.push(UserTokenPolicy {
                policy_id: "anonymous".to_string(),
                token_type: UserTokenType::Anonymous,
                issued_token_type: None,
                issuer_endpoint_url: None,
                security_policy_uri: None,
            });
        }

        if self.config.allow_user_password {
            policies.push(UserTokenPolicy {
                policy_id: "username".to_string(),
                token_type: UserTokenType::UserName,
                issued_token_type: None,
                issuer_endpoint_url: None,
                security_policy_uri: Some(
                    "http://opcfoundation.org/UA/SecurityPolicy#Basic256Sha256".to_string(),
                ),
            });
        }

        if self.config.allow_certificate {
            policies.push(UserTokenPolicy {
                policy_id: "certificate".to_string(),
                token_type: UserTokenType::Certificate,
                issued_token_type: None,
                issuer_endpoint_url: None,
                security_policy_uri: None,
            });
        }

        if self.config.allow_issued_token {
            for token_type in &self.config.accepted_token_types {
                policies.push(UserTokenPolicy {
                    policy_id: format!("issued_{}", token_type.replace('/', "_")),
                    token_type: UserTokenType::IssuedToken,
                    issued_token_type: Some(token_type.clone()),
                    issuer_endpoint_url: None,
                    security_policy_uri: None,
                });
            }
        }

        policies
    }
}

/// User token policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserTokenPolicy {
    /// Policy ID.
    pub policy_id: String,
    /// Token type.
    pub token_type: UserTokenType,
    /// Issued token type (for IssuedToken).
    pub issued_token_type: Option<String>,
    /// Issuer endpoint URL.
    pub issuer_endpoint_url: Option<String>,
    /// Security policy URI for token encryption.
    pub security_policy_uri: Option<String>,
}

// =========================================================================
// Helper Functions
// =========================================================================

/// Generate a random salt for password hashing.
fn generate_salt() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    format!("{:016X}", timestamp)
}

/// Hash a password with salt.
fn hash_password(password: &str, salt: &str) -> String {
    // Simple hash for simulation (use proper bcrypt/argon2 in production)
    let combined = format!("{}:{}", salt, password);
    let mut hash = [0u8; 32];

    for (i, byte) in combined.bytes().enumerate() {
        hash[i % 32] = hash[i % 32].wrapping_add(byte);
    }

    hash.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anonymous_auth() {
        let authenticator = UserAuthenticator::new(UserAuthConfig::default());
        let result = authenticator.authenticate(&UserCredentials::Anonymous);

        assert!(result.success);
        assert_eq!(result.user_identity, "Anonymous");
    }

    #[test]
    fn test_anonymous_disabled() {
        let config = UserAuthConfig {
            allow_anonymous: false,
            ..Default::default()
        };
        let authenticator = UserAuthenticator::new(config);
        let result = authenticator.authenticate(&UserCredentials::Anonymous);

        assert!(!result.success);
    }

    #[test]
    fn test_user_password_auth() {
        let authenticator = UserAuthenticator::new(UserAuthConfig::default());

        // Add user
        authenticator.add_user(UserAccount::new("testuser", "Password123"));

        // Successful auth
        let result =
            authenticator.authenticate(&UserCredentials::user_password("testuser", "Password123"));
        assert!(result.success);
        assert_eq!(result.user_identity, "testuser");

        // Failed auth
        let result = authenticator
            .authenticate(&UserCredentials::user_password("testuser", "WrongPassword"));
        assert!(!result.success);
    }

    #[test]
    fn test_account_lockout() {
        let config = UserAuthConfig {
            lockout_threshold: 3,
            lockout_duration_minutes: 30,
            ..Default::default()
        };
        let authenticator = UserAuthenticator::new(config);
        authenticator.add_user(UserAccount::new("testuser", "Password123"));

        // Fail 3 times
        for _ in 0..3 {
            let result = authenticator
                .authenticate(&UserCredentials::user_password("testuser", "WrongPassword"));
            assert!(!result.success);
        }

        // Account should be locked
        let result =
            authenticator.authenticate(&UserCredentials::user_password("testuser", "Password123"));
        assert!(!result.success);
        assert!(result.error_message.unwrap().contains("locked"));
    }

    #[test]
    fn test_password_validation() {
        let config = UserAuthConfig {
            password_min_length: 8,
            password_require_uppercase: true,
            password_require_number: true,
            password_require_special: false,
            ..Default::default()
        };
        let authenticator = UserAuthenticator::new(config);

        assert!(authenticator.validate_password("Password1").is_ok());
        assert!(authenticator.validate_password("short").is_err());
        assert!(authenticator.validate_password("nouppercase1").is_err());
        assert!(authenticator.validate_password("NoNumber").is_err());
    }

    #[test]
    fn test_token_policies() {
        let authenticator = UserAuthenticator::new(UserAuthConfig::default());
        let policies = authenticator.token_policies();

        // Should have anonymous, username, certificate
        assert!(policies
            .iter()
            .any(|p| p.token_type == UserTokenType::Anonymous));
        assert!(policies
            .iter()
            .any(|p| p.token_type == UserTokenType::UserName));
        assert!(policies
            .iter()
            .any(|p| p.token_type == UserTokenType::Certificate));
    }

    #[test]
    fn test_user_account() {
        let mut account = UserAccount::new("test", "password");

        assert!(account.verify_password("password"));
        assert!(!account.verify_password("wrong"));
        assert!(!account.is_locked());
        assert!(!account.is_password_expired());

        // Test login recording
        account.record_login_success();
        assert!(account.last_login.is_some());
        assert_eq!(account.failed_login_count, 0);
    }
}
