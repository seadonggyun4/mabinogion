//! OPC UA configuration types.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Transport protocol for OPC UA endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportProtocol {
    /// UA-TCP binary protocol.
    OpcTcp,
    /// HTTPS transport for request/response service dispatch.
    Https,
}

impl Default for TransportProtocol {
    fn default() -> Self {
        Self::OpcTcp
    }
}

impl TransportProtocol {
    /// URI scheme for the transport.
    pub fn scheme(&self) -> &'static str {
        match self {
            Self::OpcTcp => "opc.tcp",
            Self::Https => "https",
        }
    }
}

/// Connection initiation mode for an OPC UA transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportConnectionMode {
    /// The server listens for inbound client connections.
    Listener,
    /// The server establishes an outbound UA-TCP connection to a remote listener.
    ReverseConnect,
}

impl Default for TransportConnectionMode {
    fn default() -> Self {
        Self::Listener
    }
}

impl TransportConnectionMode {
    /// Stable string form used in summaries and diagnostics.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Listener => "listener",
            Self::ReverseConnect => "reverse_connect",
        }
    }
}

/// Security policy for OPC UA.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SecurityPolicy {
    /// No security.
    None,
    /// Basic128Rsa15.
    Basic128Rsa15,
    /// Basic256.
    Basic256,
    /// Basic256Sha256.
    Basic256Sha256,
    /// Aes128Sha256RsaOaep.
    Aes128Sha256RsaOaep,
    /// Aes256Sha256RsaPss.
    Aes256Sha256RsaPss,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self::None
    }
}

impl SecurityPolicy {
    /// Get the URI for this security policy.
    pub fn uri(&self) -> &'static str {
        match self {
            Self::None => "http://opcfoundation.org/UA/SecurityPolicy#None",
            Self::Basic128Rsa15 => "http://opcfoundation.org/UA/SecurityPolicy#Basic128Rsa15",
            Self::Basic256 => "http://opcfoundation.org/UA/SecurityPolicy#Basic256",
            Self::Basic256Sha256 => "http://opcfoundation.org/UA/SecurityPolicy#Basic256Sha256",
            Self::Aes128Sha256RsaOaep => {
                "http://opcfoundation.org/UA/SecurityPolicy#Aes128_Sha256_RsaOaep"
            }
            Self::Aes256Sha256RsaPss => {
                "http://opcfoundation.org/UA/SecurityPolicy#Aes256_Sha256_RsaPss"
            }
        }
    }
}

/// Message security mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageSecurityMode {
    /// Invalid mode.
    Invalid,
    /// No security.
    None,
    /// Messages are signed.
    Sign,
    /// Messages are signed and encrypted.
    SignAndEncrypt,
}

impl Default for MessageSecurityMode {
    fn default() -> Self {
        Self::None
    }
}

/// User token configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserTokenConfig {
    /// Token policy ID.
    pub policy_id: String,
    /// Token type.
    pub token_type: UserTokenType,
    /// Issued token type (for IssuedToken).
    pub issued_token_type: Option<String>,
    /// Issuer endpoint URL.
    pub issuer_endpoint_url: Option<String>,
    /// Security policy URI.
    pub security_policy_uri: Option<String>,
}

impl Default for UserTokenConfig {
    fn default() -> Self {
        Self {
            policy_id: "anonymous".to_string(),
            token_type: UserTokenType::Anonymous,
            issued_token_type: None,
            issuer_endpoint_url: None,
            security_policy_uri: None,
        }
    }
}

/// User token types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserTokenType {
    /// Anonymous token.
    Anonymous,
    /// Username/password token.
    UserName,
    /// X.509 certificate token.
    Certificate,
    /// Issued token (e.g., JWT).
    IssuedToken,
}

impl Default for UserTokenType {
    fn default() -> Self {
        Self::Anonymous
    }
}

/// Endpoint configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointConfig {
    /// Transport protocol.
    #[serde(default)]
    pub protocol: TransportProtocol,
    /// Endpoint path (e.g., "/opcua").
    pub path: String,
    /// Security policy.
    pub security_policy: SecurityPolicy,
    /// Message security mode.
    pub security_mode: MessageSecurityMode,
    /// User token policies.
    pub user_token_policies: Vec<UserTokenConfig>,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            protocol: TransportProtocol::OpcTcp,
            path: "/opcua".to_string(),
            security_policy: SecurityPolicy::None,
            security_mode: MessageSecurityMode::None,
            user_token_policies: vec![UserTokenConfig::default()],
        }
    }
}

impl EndpointConfig {
    /// Create a new endpoint with no security.
    pub fn no_security(path: impl Into<String>) -> Self {
        Self {
            protocol: TransportProtocol::OpcTcp,
            path: path.into(),
            security_policy: SecurityPolicy::None,
            security_mode: MessageSecurityMode::None,
            user_token_policies: vec![UserTokenConfig::default()],
        }
    }

    /// Create a new endpoint with sign mode.
    pub fn signed(path: impl Into<String>, policy: SecurityPolicy) -> Self {
        Self {
            protocol: TransportProtocol::OpcTcp,
            path: path.into(),
            security_policy: policy,
            security_mode: MessageSecurityMode::Sign,
            user_token_policies: vec![UserTokenConfig::default()],
        }
    }

    /// Create a new endpoint with sign and encrypt mode.
    pub fn encrypted(path: impl Into<String>, policy: SecurityPolicy) -> Self {
        Self {
            protocol: TransportProtocol::OpcTcp,
            path: path.into(),
            security_policy: policy,
            security_mode: MessageSecurityMode::SignAndEncrypt,
            user_token_policies: vec![UserTokenConfig::default()],
        }
    }
}

/// OPC UA server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpcUaServerConfig {
    /// Endpoint URL.
    #[serde(default = "default_endpoint")]
    pub endpoint_url: String,
    /// Canonical transport protocol for the endpoint.
    #[serde(default)]
    pub endpoint_protocol: TransportProtocol,
    /// Connection initiation mode.
    #[serde(default)]
    pub connection_mode: TransportConnectionMode,
    /// Reverse-connect target URL when `connection_mode` is `reverse_connect`.
    #[serde(default)]
    pub reverse_connect_target: Option<String>,
    /// Fixed retry interval for reverse-connect reconnect attempts.
    #[serde(default = "default_retry_interval_ms")]
    pub retry_interval_ms: u64,

    /// Server name.
    #[serde(default = "default_server_name")]
    pub server_name: String,

    /// Security policy.
    #[serde(default = "default_security_policy")]
    pub security_policy: String,

    /// Certificate path.
    pub certificate_path: Option<PathBuf>,

    /// Private key path.
    pub private_key_path: Option<PathBuf>,

    /// Maximum subscriptions.
    #[serde(default = "default_max_subscriptions")]
    pub max_subscriptions: usize,

    /// Maximum monitored items per subscription.
    #[serde(default = "default_max_monitored_items")]
    pub max_monitored_items: usize,

    /// Minimum publishing interval in milliseconds.
    #[serde(default = "default_min_publishing_interval")]
    pub min_publishing_interval_ms: u32,
}

fn default_endpoint() -> String {
    "opc.tcp://0.0.0.0:4840".to_string()
}

fn default_server_name() -> String {
    "TRAP Simulator OPC UA Server".to_string()
}

fn default_security_policy() -> String {
    "None".to_string()
}

fn default_retry_interval_ms() -> u64 {
    5_000
}

fn default_max_subscriptions() -> usize {
    100
}

fn default_max_monitored_items() -> usize {
    10000
}

fn default_min_publishing_interval() -> u32 {
    100
}

impl Default for OpcUaServerConfig {
    fn default() -> Self {
        Self {
            endpoint_url: default_endpoint(),
            endpoint_protocol: TransportProtocol::OpcTcp,
            connection_mode: TransportConnectionMode::Listener,
            reverse_connect_target: None,
            retry_interval_ms: default_retry_interval_ms(),
            server_name: default_server_name(),
            security_policy: default_security_policy(),
            certificate_path: None,
            private_key_path: None,
            max_subscriptions: default_max_subscriptions(),
            max_monitored_items: default_max_monitored_items(),
            min_publishing_interval_ms: default_min_publishing_interval(),
        }
    }
}
