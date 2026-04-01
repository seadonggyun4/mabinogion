use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;

use parking_lot::Mutex;

use super::SecurityAuditSink;
use crate::config::SecurityPolicy;
use crate::security::manager::{
    SecurityAuditSinkKind, SecurityAuditStatus, SecurityContext, SecurityManagerConfig,
};

pub(crate) struct NoopSecurityAuditSink;

impl SecurityAuditSink for NoopSecurityAuditSink {
    fn status(&self) -> SecurityAuditStatus {
        SecurityAuditStatus {
            sink_kind: SecurityAuditSinkKind::Noop,
            event_count: 0,
            last_write_status: "disabled".to_string(),
            output_path: None,
        }
    }
}

pub(crate) struct MemorySecurityAuditSink {
    events: Mutex<Vec<String>>,
}

impl MemorySecurityAuditSink {
    pub(crate) fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    fn push(&self, event: impl Into<String>) {
        self.events.lock().push(event.into());
    }
}

impl SecurityAuditSink for MemorySecurityAuditSink {
    fn on_initialized(&self, policies: &[SecurityPolicy]) {
        self.push(format!("initialized:{:?}", policies));
    }

    fn on_certificate_validated(&self, thumbprint: Option<&str>, is_valid: bool) {
        self.push(format!(
            "certificate_validated:{}:{}",
            thumbprint.unwrap_or("-"),
            is_valid
        ));
    }

    fn on_authentication(&self, token_type: &str, success: bool) {
        self.push(format!("authentication:{}:{}", token_type, success));
    }

    fn on_deprecated_policy_warning(&self, policy: SecurityPolicy) {
        self.push(format!("deprecated_policy_warning:{:?}", policy));
    }

    fn on_secure_channel_created(&self, context: &SecurityContext) {
        self.push(format!("channel_created:{}", context.secure_channel_id));
    }

    fn on_secure_channel_renewed(&self, context: &SecurityContext) {
        self.push(format!("channel_renewed:{}", context.secure_channel_id));
    }

    fn on_secure_channel_closed(&self, channel_id: u32) {
        self.push(format!("channel_closed:{}", channel_id));
    }

    fn on_secure_channel_cleanup(&self, count: usize) {
        self.push(format!("channel_cleanup:{}", count));
    }

    fn on_trust_store_reloaded(&self, trusted: usize, rejected: usize) {
        self.push(format!("trust_store_reloaded:{}:{}", trusted, rejected));
    }

    fn on_server_certificate_rotated(&self, thumbprint: Option<&str>) {
        self.push(format!(
            "server_certificate_rotated:{}",
            thumbprint.unwrap_or("-")
        ));
    }

    fn status(&self) -> SecurityAuditStatus {
        let events = self.events.lock();
        SecurityAuditStatus {
            sink_kind: SecurityAuditSinkKind::Memory,
            event_count: events.len(),
            last_write_status: events.last().cloned().unwrap_or_else(|| "idle".to_string()),
            output_path: None,
        }
    }
}

pub(crate) struct JsonlSecurityAuditSink {
    path: std::path::PathBuf,
    state: Mutex<JsonlAuditState>,
}

#[derive(Debug, Default)]
struct JsonlAuditState {
    event_count: usize,
    last_write_status: String,
}

impl JsonlSecurityAuditSink {
    pub(crate) fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            state: Mutex::new(JsonlAuditState {
                event_count: 0,
                last_write_status: "idle".to_string(),
            }),
        }
    }

    fn append(&self, event: serde_json::Value) {
        let Some(parent) = self.path.parent() else {
            self.state.lock().last_write_status = "error: missing_parent".to_string();
            return;
        };
        if std::fs::create_dir_all(parent).is_err() {
            self.state.lock().last_write_status = "error: create_dir_all".to_string();
            return;
        }
        let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        else {
            self.state.lock().last_write_status = "error: open_failed".to_string();
            return;
        };
        match writeln!(file, "{}", event) {
            Ok(()) => {
                let mut state = self.state.lock();
                state.event_count += 1;
                state.last_write_status = "ok".to_string();
            }
            Err(error) => {
                self.state.lock().last_write_status = format!("error: {}", error);
            }
        }
    }
}

impl SecurityAuditSink for JsonlSecurityAuditSink {
    fn on_initialized(&self, policies: &[SecurityPolicy]) {
        self.append(serde_json::json!({ "event": "initialized", "policies": policies }));
    }

    fn on_certificate_validated(&self, thumbprint: Option<&str>, is_valid: bool) {
        self.append(serde_json::json!({
            "event": "certificate_validated",
            "thumbprint": thumbprint,
            "is_valid": is_valid,
        }));
    }

    fn on_authentication(&self, token_type: &str, success: bool) {
        self.append(serde_json::json!({
            "event": "authentication",
            "token_type": token_type,
            "success": success,
        }));
    }

    fn on_deprecated_policy_warning(&self, policy: SecurityPolicy) {
        self.append(serde_json::json!({
            "event": "deprecated_policy_warning",
            "policy": format!("{:?}", policy),
        }));
    }

    fn on_secure_channel_created(&self, context: &SecurityContext) {
        self.append(serde_json::json!({
            "event": "channel_created",
            "channel_id": context.secure_channel_id,
            "token_id": context.token_id,
        }));
    }

    fn on_secure_channel_renewed(&self, context: &SecurityContext) {
        self.append(serde_json::json!({
            "event": "channel_renewed",
            "channel_id": context.secure_channel_id,
            "token_id": context.token_id,
        }));
    }

    fn on_secure_channel_closed(&self, channel_id: u32) {
        self.append(serde_json::json!({
            "event": "channel_closed",
            "channel_id": channel_id,
        }));
    }

    fn on_secure_channel_cleanup(&self, count: usize) {
        self.append(serde_json::json!({
            "event": "channel_cleanup",
            "count": count,
        }));
    }

    fn on_trust_store_reloaded(&self, trusted: usize, rejected: usize) {
        self.append(serde_json::json!({
            "event": "trust_store_reloaded",
            "trusted": trusted,
            "rejected": rejected,
        }));
    }

    fn on_server_certificate_rotated(&self, thumbprint: Option<&str>) {
        self.append(serde_json::json!({
            "event": "server_certificate_rotated",
            "thumbprint": thumbprint,
        }));
    }

    fn status(&self) -> SecurityAuditStatus {
        let state = self.state.lock();
        SecurityAuditStatus {
            sink_kind: SecurityAuditSinkKind::JsonlFile,
            event_count: state.event_count,
            last_write_status: state.last_write_status.clone(),
            output_path: Some(self.path.clone()),
        }
    }
}

pub(crate) fn build_audit_sink(config: &SecurityManagerConfig) -> Arc<dyn SecurityAuditSink> {
    match config.audit_sink.kind {
        SecurityAuditSinkKind::Noop => Arc::new(NoopSecurityAuditSink),
        SecurityAuditSinkKind::Memory => Arc::new(MemorySecurityAuditSink::new()),
        SecurityAuditSinkKind::JsonlFile => Arc::new(JsonlSecurityAuditSink::new(
            config
                .audit_sink
                .path
                .clone()
                .unwrap_or_else(|| std::env::temp_dir().join("mabi-opcua-audit.jsonl")),
        )),
    }
}
