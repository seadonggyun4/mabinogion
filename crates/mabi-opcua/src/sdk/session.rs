//! OPC UA session management.
//!
//! Sessions represent authenticated connections from clients to the server.

use std::sync::atomic::{AtomicU32, Ordering};

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::types::NodeId;

/// Session manager configuration.
#[derive(Debug, Clone)]
pub struct SessionManagerConfig {
    /// Maximum number of sessions.
    pub max_sessions: usize,
    /// Session timeout in milliseconds.
    pub session_timeout_ms: u64,
    /// Maximum subscriptions per session.
    pub max_subscriptions_per_session: usize,
}

impl Default for SessionManagerConfig {
    fn default() -> Self {
        Self {
            max_sessions: 1000,
            session_timeout_ms: 60_000, // 1 minute
            max_subscriptions_per_session: 100,
        }
    }
}

/// Type alias for backward compatibility.
pub type SessionConfig = SessionManagerConfig;

/// A session instance.
///
/// Represents an active connection from a client.
pub struct Session {
    /// Session information.
    info: SessionInfo,
}

impl Session {
    /// Create a new session.
    pub fn new(info: SessionInfo) -> Self {
        Self { info }
    }

    /// Get the session ID.
    pub fn id(&self) -> &NodeId {
        &self.info.session_id
    }

    /// Get the session info.
    pub fn info(&self) -> &SessionInfo {
        &self.info
    }

    /// Get mutable session info.
    pub fn info_mut(&mut self) -> &mut SessionInfo {
        &mut self.info
    }

    /// Check if session is active.
    pub fn is_active(&self) -> bool {
        self.info.is_active()
    }

    /// Check if session is timed out.
    pub fn is_timed_out(&self) -> bool {
        self.info.is_timed_out()
    }

    /// Update last activity.
    pub fn touch(&mut self) {
        self.info.touch();
    }

    /// Activate the session.
    pub fn activate(&mut self, user_identity: UserIdentity) {
        self.info.activate(user_identity);
    }

    /// Close the session.
    pub fn close(&mut self) {
        self.info.close();
    }
}

/// User identity type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserIdentity {
    /// Anonymous user.
    Anonymous,
    /// Username/password authentication.
    UserName {
        username: String,
        // Note: Password is not stored
    },
    /// X.509 certificate authentication.
    Certificate { thumbprint: String, subject: String },
    /// Issued token (e.g., JWT).
    IssuedToken { token_type: String },
}

impl Default for UserIdentity {
    fn default() -> Self {
        Self::Anonymous
    }
}

/// Session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Session created but not activated.
    Created,
    /// Session is active.
    Active,
    /// Session is closed.
    Closed,
    /// Session timed out.
    TimedOut,
}

/// Session information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session ID.
    pub session_id: NodeId,
    /// Authentication token.
    pub authentication_token: NodeId,
    /// Session name.
    pub session_name: String,
    /// Client description.
    pub client_description: String,
    /// Server URI.
    pub server_uri: String,
    /// Endpoint URL.
    pub endpoint_url: String,
    /// Security policy URI.
    pub security_policy_uri: String,
    /// Security mode.
    pub security_mode: String,
    /// User identity.
    pub user_identity: UserIdentity,
    /// Session state.
    pub state: SessionState,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// Last activity time.
    pub last_activity: DateTime<Utc>,
    /// Timeout in milliseconds.
    pub timeout_ms: u64,
    /// Subscription IDs.
    pub subscriptions: Vec<u32>,
    /// Maximum response message size.
    pub max_response_message_size: u32,
}

impl SessionInfo {
    /// Create a new session info.
    pub fn new(session_id: NodeId, session_name: impl Into<String>, timeout_ms: u64) -> Self {
        let now = Utc::now();
        Self {
            session_id: session_id.clone(),
            authentication_token: NodeId::next_numeric(0),
            session_name: session_name.into(),
            client_description: String::new(),
            server_uri: String::new(),
            endpoint_url: String::new(),
            security_policy_uri: String::new(),
            security_mode: String::new(),
            user_identity: UserIdentity::Anonymous,
            state: SessionState::Created,
            created_at: now,
            last_activity: now,
            timeout_ms,
            subscriptions: Vec::new(),
            max_response_message_size: 0,
        }
    }

    /// Check if session is timed out.
    pub fn is_timed_out(&self) -> bool {
        let timeout = Duration::milliseconds(self.timeout_ms as i64);
        Utc::now() - self.last_activity > timeout
    }

    /// Check if session is active.
    pub fn is_active(&self) -> bool {
        self.state == SessionState::Active && !self.is_timed_out()
    }

    /// Update last activity time.
    pub fn touch(&mut self) {
        self.last_activity = Utc::now();
    }

    /// Activate the session.
    pub fn activate(&mut self, user_identity: UserIdentity) {
        self.state = SessionState::Active;
        self.user_identity = user_identity;
        self.touch();
    }

    /// Close the session.
    pub fn close(&mut self) {
        self.state = SessionState::Closed;
    }

    /// Add a subscription.
    pub fn add_subscription(&mut self, subscription_id: u32) {
        if !self.subscriptions.contains(&subscription_id) {
            self.subscriptions.push(subscription_id);
        }
    }

    /// Remove a subscription.
    pub fn remove_subscription(&mut self, subscription_id: u32) {
        self.subscriptions.retain(|&id| id != subscription_id);
    }
}

/// Session event.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Session created.
    Created { session_id: NodeId },
    /// Session activated.
    Activated {
        session_id: NodeId,
        user: UserIdentity,
    },
    /// Session closed.
    Closed { session_id: NodeId },
    /// Session timed out.
    TimedOut { session_id: NodeId },
}

/// Session manager.
///
/// Manages all active sessions and their lifecycle.
pub struct SessionManager {
    config: SessionManagerConfig,
    sessions: DashMap<NodeId, SessionInfo>,
    session_counter: AtomicU32,
    event_tx: broadcast::Sender<SessionEvent>,
}

impl SessionManager {
    /// Create a new session manager with default config.
    pub fn new() -> Self {
        Self::with_config(SessionManagerConfig::default())
    }

    /// Create a new session manager with config.
    pub fn with_config(config: SessionManagerConfig) -> Self {
        let (event_tx, _) = broadcast::channel(256);

        Self {
            config,
            sessions: DashMap::new(),
            session_counter: AtomicU32::new(1),
            event_tx,
        }
    }

    /// Create a new session.
    pub fn create_session(
        &self,
        session_name: impl Into<String>,
    ) -> Result<SessionInfo, SessionError> {
        if self.sessions.len() >= self.config.max_sessions {
            return Err(SessionError::MaxSessionsReached);
        }

        let session_id = self.next_session_id();
        let session = SessionInfo::new(
            session_id.clone(),
            session_name,
            self.config.session_timeout_ms,
        );

        self.sessions.insert(session_id.clone(), session.clone());

        info!(session_id = %session_id, "Session created");
        let _ = self.event_tx.send(SessionEvent::Created { session_id });

        Ok(session)
    }

    /// Activate a session.
    pub fn activate_session(
        &self,
        session_id: &NodeId,
        user_identity: UserIdentity,
    ) -> Result<(), SessionError> {
        let mut session = self
            .sessions
            .get_mut(session_id)
            .ok_or(SessionError::SessionNotFound)?;

        if session.state != SessionState::Created {
            return Err(SessionError::InvalidState);
        }

        session.activate(user_identity.clone());

        info!(session_id = %session_id, "Session activated");
        let _ = self.event_tx.send(SessionEvent::Activated {
            session_id: session_id.clone(),
            user: user_identity,
        });

        Ok(())
    }

    /// Close a session.
    pub fn close_session(&self, session_id: &NodeId) -> Result<(), SessionError> {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            session.close();
            drop(session);

            // Remove after updating state
            self.sessions.remove(session_id);

            info!(session_id = %session_id, "Session closed");
            let _ = self.event_tx.send(SessionEvent::Closed {
                session_id: session_id.clone(),
            });

            Ok(())
        } else {
            Err(SessionError::SessionNotFound)
        }
    }

    /// Get a session.
    pub fn get_session(&self, session_id: &NodeId) -> Option<SessionInfo> {
        self.sessions.get(session_id).map(|s| s.clone())
    }

    /// Get session by authentication token.
    pub fn get_session_by_token(&self, auth_token: &NodeId) -> Option<SessionInfo> {
        self.sessions
            .iter()
            .find(|e| &e.authentication_token == auth_token)
            .map(|e| e.clone())
    }

    /// Update session activity.
    pub fn touch_session(&self, session_id: &NodeId) {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            session.touch();
        }
    }

    /// Add subscription to session.
    pub fn add_subscription(
        &self,
        session_id: &NodeId,
        subscription_id: u32,
    ) -> Result<(), SessionError> {
        let mut session = self
            .sessions
            .get_mut(session_id)
            .ok_or(SessionError::SessionNotFound)?;

        if session.subscriptions.len() >= self.config.max_subscriptions_per_session {
            return Err(SessionError::TooManySubscriptions);
        }

        session.add_subscription(subscription_id);
        Ok(())
    }

    /// Remove subscription from session.
    pub fn remove_subscription(&self, session_id: &NodeId, subscription_id: u32) {
        if let Some(mut session) = self.sessions.get_mut(session_id) {
            session.remove_subscription(subscription_id);
        }
    }

    /// Cleanup expired sessions.
    pub fn cleanup_expired(&self) {
        let expired: Vec<NodeId> = self
            .sessions
            .iter()
            .filter(|e| e.is_timed_out())
            .map(|e| e.session_id.clone())
            .collect();

        for session_id in expired {
            if let Some((_, session)) = self.sessions.remove(&session_id) {
                warn!(session_id = %session_id, "Session timed out");
                let _ = self.event_tx.send(SessionEvent::TimedOut {
                    session_id: session.session_id,
                });
            }
        }
    }

    /// Get the number of active sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Get all session IDs.
    pub fn session_ids(&self) -> Vec<NodeId> {
        self.sessions.iter().map(|e| e.key().clone()).collect()
    }

    /// Subscribe to session events.
    pub fn subscribe_events(&self) -> broadcast::Receiver<SessionEvent> {
        self.event_tx.subscribe()
    }

    /// Generate next session ID.
    fn next_session_id(&self) -> NodeId {
        let id = self.session_counter.fetch_add(1, Ordering::SeqCst);
        NodeId::numeric(1, id)
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Session error types.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SessionError {
    #[error("Maximum sessions reached")]
    MaxSessionsReached,
    #[error("Session not found")]
    SessionNotFound,
    #[error("Invalid session state")]
    InvalidState,
    #[error("Too many subscriptions")]
    TooManySubscriptions,
    #[error("Session timed out")]
    TimedOut,
    #[error("Not authorized")]
    NotAuthorized,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_session() {
        let manager = SessionManager::default();

        let session = manager.create_session("TestSession").unwrap();
        assert_eq!(session.state, SessionState::Created);
        assert_eq!(manager.session_count(), 1);
    }

    #[test]
    fn test_activate_session() {
        let manager = SessionManager::default();

        let session = manager.create_session("TestSession").unwrap();
        manager
            .activate_session(&session.session_id, UserIdentity::Anonymous)
            .unwrap();

        let updated = manager.get_session(&session.session_id).unwrap();
        assert_eq!(updated.state, SessionState::Active);
    }

    #[test]
    fn test_close_session() {
        let manager = SessionManager::default();

        let session = manager.create_session("TestSession").unwrap();
        manager.close_session(&session.session_id).unwrap();

        assert!(manager.get_session(&session.session_id).is_none());
        assert_eq!(manager.session_count(), 0);
    }

    #[test]
    fn test_max_sessions() {
        let manager = SessionManager::with_config(SessionManagerConfig {
            max_sessions: 2,
            ..Default::default()
        });

        manager.create_session("Session1").unwrap();
        manager.create_session("Session2").unwrap();

        let result = manager.create_session("Session3");
        assert!(matches!(result, Err(SessionError::MaxSessionsReached)));
    }

    #[test]
    fn test_session_subscriptions() {
        let manager = SessionManager::default();

        let session = manager.create_session("TestSession").unwrap();
        manager.add_subscription(&session.session_id, 1).unwrap();
        manager.add_subscription(&session.session_id, 2).unwrap();

        let updated = manager.get_session(&session.session_id).unwrap();
        assert_eq!(updated.subscriptions.len(), 2);

        manager.remove_subscription(&session.session_id, 1);

        let updated = manager.get_session(&session.session_id).unwrap();
        assert_eq!(updated.subscriptions.len(), 1);
    }
}
