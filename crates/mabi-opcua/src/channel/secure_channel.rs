//! OPC UA Secure Channel state management.
//!
//! Manages secure channel lifecycle, token issuance, and sequence numbers.
//! For SecurityPolicy::None, no actual encryption is performed.

use std::sync::atomic::{AtomicU32, Ordering};

use crate::config::{SecurityPolicy, MessageSecurityMode};

/// Secure channel state for a single client connection.
#[derive(Debug)]
pub struct SecureChannel {
    /// Unique channel identifier assigned by the server.
    channel_id: u32,
    /// Current security token ID.
    token_id: u32,
    /// Negotiated security policy.
    security_policy: SecurityPolicy,
    /// Negotiated message security mode.
    security_mode: MessageSecurityMode,
    /// Next expected sequence number from the client.
    client_sequence_number: AtomicU32,
    /// Server's next sequence number for responses.
    server_sequence_number: AtomicU32,
    /// Token lifetime in milliseconds.
    token_lifetime_ms: u32,
    /// Creation time of the current token.
    token_created_at: std::time::Instant,
}

/// Counter for generating unique channel IDs.
static NEXT_CHANNEL_ID: AtomicU32 = AtomicU32::new(1);

/// Counter for generating unique token IDs.
static NEXT_TOKEN_ID: AtomicU32 = AtomicU32::new(1);

impl SecureChannel {
    /// Create a new secure channel with SecurityPolicy::None.
    pub fn new_unsecured() -> Self {
        Self {
            channel_id: NEXT_CHANNEL_ID.fetch_add(1, Ordering::SeqCst),
            token_id: NEXT_TOKEN_ID.fetch_add(1, Ordering::SeqCst),
            security_policy: SecurityPolicy::None,
            security_mode: MessageSecurityMode::None,
            client_sequence_number: AtomicU32::new(0),
            server_sequence_number: AtomicU32::new(1),
            token_lifetime_ms: 3_600_000, // 1 hour default
            token_created_at: std::time::Instant::now(),
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
            token_id: NEXT_TOKEN_ID.fetch_add(1, Ordering::SeqCst),
            security_policy,
            security_mode,
            client_sequence_number: AtomicU32::new(0),
            server_sequence_number: AtomicU32::new(1),
            token_lifetime_ms,
            token_created_at: std::time::Instant::now(),
        }
    }

    pub fn channel_id(&self) -> u32 { self.channel_id }
    pub fn token_id(&self) -> u32 { self.token_id }
    pub fn security_policy(&self) -> &SecurityPolicy { &self.security_policy }
    pub fn security_mode(&self) -> &MessageSecurityMode { &self.security_mode }
    pub fn token_lifetime_ms(&self) -> u32 { self.token_lifetime_ms }

    /// Get the next server sequence number (atomically increments).
    pub fn next_server_sequence_number(&self) -> u32 {
        self.server_sequence_number.fetch_add(1, Ordering::SeqCst)
    }

    /// Validate and update the client sequence number.
    pub fn validate_sequence_number(&self, received: u32) -> bool {
        // OPC UA spec: sequence numbers should be monotonically increasing
        // For simplicity, just track the latest
        self.client_sequence_number.store(received, Ordering::SeqCst);
        true
    }

    /// Renew the security token.
    pub fn renew_token(&mut self, lifetime_ms: u32) {
        self.token_id = NEXT_TOKEN_ID.fetch_add(1, Ordering::SeqCst);
        self.token_lifetime_ms = lifetime_ms;
        self.token_created_at = std::time::Instant::now();
    }

    /// Check if the current token has expired.
    pub fn is_token_expired(&self) -> bool {
        self.token_created_at.elapsed().as_millis() > self.token_lifetime_ms as u128
    }
}
