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
use crate::types::NodeId;

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
    /// Attached session ID for this channel, if any.
    attached_session_id: RwLock<Option<NodeId>>,
    /// Attached authentication token for this channel, if any.
    attached_auth_token: RwLock<Option<NodeId>>,
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
            attached_session_id: RwLock::new(None),
            attached_auth_token: RwLock::new(None),
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
            attached_session_id: RwLock::new(None),
            attached_auth_token: RwLock::new(None),
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
        let previous = self.client_sequence_number.load(Ordering::SeqCst);
        if previous != 0 && received <= previous {
            return false;
        }
        self.client_sequence_number
            .store(received, Ordering::SeqCst);
        true
    }

    /// Get the next expected client sequence number.
    pub fn expected_client_sequence_number(&self) -> u32 {
        match self.client_sequence_number.load(Ordering::SeqCst) {
            0 => 1,
            previous => previous.saturating_add(1),
        }
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

    /// Check whether the supplied token matches the current token.
    pub fn matches_token(&self, token_id: u32) -> bool {
        self.token_id() == token_id
    }

    /// Attach a session to this channel.
    pub fn attach_session(&self, session_id: NodeId, auth_token: NodeId) {
        *self.attached_session_id.write() = Some(session_id);
        *self.attached_auth_token.write() = Some(auth_token);
    }

    /// Clear any attached session metadata.
    pub fn clear_session(&self) {
        *self.attached_session_id.write() = None;
        *self.attached_auth_token.write() = None;
    }

    /// Get the attached session ID, if any.
    pub fn session_id(&self) -> Option<NodeId> {
        self.attached_session_id.read().clone()
    }

    /// Get the attached auth token, if any.
    pub fn auth_token(&self) -> Option<NodeId> {
        self.attached_auth_token.read().clone()
    }
}
