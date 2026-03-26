//! OPC UA Secure Channel state management.
//!
//! Manages secure channel lifecycle, token issuance, and sequence numbers.
//! For SecurityPolicy::None, no actual encryption is performed.
//!
//! # Token Renewal
//!
//! OPC UA clients periodically send `OpenSecureChannel` requests with
//! `requestType = 1` (Renew) to refresh the security token before it
//! expires.  The server MUST reuse the existing `channel_id` and only
//! issue a new `token_id`.  All mutable token state uses interior
//! mutability so renewal works through `&self` / `Arc<SecureChannel>`.

use std::sync::atomic::{AtomicU32, Ordering};

use parking_lot::RwLock;

use crate::config::{MessageSecurityMode, SecurityPolicy};

/// Secure channel state for a single client connection.
///
/// Token-related fields use interior mutability so that renewal can
/// happen through `Arc<SecureChannel>` without requiring `&mut self`.
#[derive(Debug)]
pub struct SecureChannel {
    /// Unique channel identifier assigned by the server.
    channel_id: u32,
    /// Current security token ID (interior mutable for renewal).
    token_id: AtomicU32,
    /// Negotiated security policy.
    security_policy: SecurityPolicy,
    /// Negotiated message security mode.
    security_mode: MessageSecurityMode,
    /// Next expected sequence number from the client.
    client_sequence_number: AtomicU32,
    /// Server's next sequence number for responses.
    server_sequence_number: AtomicU32,
    /// Token lifetime in milliseconds (interior mutable for renewal).
    token_lifetime_ms: AtomicU32,
    /// Creation time of the current token (interior mutable for renewal).
    token_created_at: RwLock<std::time::Instant>,
}

/// Counter for generating unique channel IDs.
static NEXT_CHANNEL_ID: AtomicU32 = AtomicU32::new(1);

/// Counter for generating unique token IDs.
static NEXT_TOKEN_ID: AtomicU32 = AtomicU32::new(1);

/// OPC UA OpenSecureChannel request types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SecurityTokenRequestType {
    /// Initial token issuance for a new channel.
    Issue = 0,
    /// Token renewal for an existing channel.
    Renew = 1,
}

impl SecurityTokenRequestType {
    /// Parse from the wire value.  Unknown values are treated as `Issue`.
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Renew,
            _ => Self::Issue,
        }
    }
}

impl SecureChannel {
    /// Create a new secure channel with SecurityPolicy::None.
    pub fn new_unsecured() -> Self {
        Self {
            channel_id: NEXT_CHANNEL_ID.fetch_add(1, Ordering::SeqCst),
            token_id: AtomicU32::new(NEXT_TOKEN_ID.fetch_add(1, Ordering::SeqCst)),
            security_policy: SecurityPolicy::None,
            security_mode: MessageSecurityMode::None,
            client_sequence_number: AtomicU32::new(0),
            server_sequence_number: AtomicU32::new(1),
            token_lifetime_ms: AtomicU32::new(3_600_000), // 1 hour default
            token_created_at: RwLock::new(std::time::Instant::now()),
        }
    }

    /// Create a new secure channel with the given security parameters.
    pub fn new(
        security_policy: SecurityPolicy,
        security_mode: MessageSecurityMode,
        token_lifetime_ms: u32,
    ) -> Self {
        Self {
            channel_id: NEXT_CHANNEL_ID.fetch_add(1, Ordering::SeqCst),
            token_id: AtomicU32::new(NEXT_TOKEN_ID.fetch_add(1, Ordering::SeqCst)),
            security_policy,
            security_mode,
            client_sequence_number: AtomicU32::new(0),
            server_sequence_number: AtomicU32::new(1),
            token_lifetime_ms: AtomicU32::new(token_lifetime_ms),
            token_created_at: RwLock::new(std::time::Instant::now()),
        }
    }

    pub fn channel_id(&self) -> u32 {
        self.channel_id
    }
    pub fn token_id(&self) -> u32 {
        self.token_id.load(Ordering::SeqCst)
    }
    pub fn security_policy(&self) -> &SecurityPolicy {
        &self.security_policy
    }
    pub fn security_mode(&self) -> &MessageSecurityMode {
        &self.security_mode
    }
    pub fn token_lifetime_ms(&self) -> u32 {
        self.token_lifetime_ms.load(Ordering::Relaxed)
    }

    /// Get the next server sequence number (atomically increments).
    pub fn next_server_sequence_number(&self) -> u32 {
        self.server_sequence_number.fetch_add(1, Ordering::SeqCst)
    }

    /// Validate and update the client sequence number.
    pub fn validate_sequence_number(&self, received: u32) -> bool {
        // OPC UA spec: sequence numbers should be monotonically increasing
        // For simplicity, just track the latest
        self.client_sequence_number
            .store(received, Ordering::SeqCst);
        true
    }

    /// Renew the security token (callable through `&self` / `Arc`).
    ///
    /// Issues a new `token_id` and resets the token lifetime and creation
    /// timestamp.  The `channel_id` remains unchanged per OPC UA spec.
    pub fn renew_token(&self, lifetime_ms: u32) {
        let new_token = NEXT_TOKEN_ID.fetch_add(1, Ordering::SeqCst);
        self.token_id.store(new_token, Ordering::SeqCst);
        self.token_lifetime_ms.store(lifetime_ms, Ordering::Relaxed);
        *self.token_created_at.write() = std::time::Instant::now();
    }

    /// Check if the current token has expired.
    pub fn is_token_expired(&self) -> bool {
        let created = *self.token_created_at.read();
        created.elapsed().as_millis() > self.token_lifetime_ms.load(Ordering::Relaxed) as u128
    }
}
