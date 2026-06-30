use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::service::{RUNTIME_CONTRACT_VERSION, SNAPSHOT_METADATA_VERSION};
use crate::session::RuntimeSessionSnapshot;

/// Stable run evidence schema version consumed by Imugi and Trials.
pub const RUN_EVIDENCE_SCHEMA_VERSION: &str = "run-evidence-schema-v1";

/// Stable trial artifact contract version consumed by Imugi and Trials.
pub const TRIAL_ARTIFACT_CONTRACT_VERSION: &str = "trial-artifact-contract-v1";

/// Protocol profile identity carried through from Unified Readiness and Trials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProtocolProfileEvidence {
    pub protocol: String,
    pub profile_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_status: Option<String>,
}

impl ProtocolProfileEvidence {
    pub fn new(protocol: impl Into<String>, profile_id: impl Into<String>) -> Self {
        Self {
            protocol: protocol.into(),
            profile_id: profile_id.into(),
            capability_id: None,
            lane: None,
            coverage_status: None,
        }
    }

    pub fn with_capability_id(mut self, capability_id: impl Into<String>) -> Self {
        self.capability_id = Some(capability_id.into());
        self
    }

    pub fn with_lane(mut self, lane: impl Into<String>) -> Self {
        self.lane = Some(lane.into());
        self
    }

    pub fn with_coverage_status(mut self, coverage_status: impl Into<String>) -> Self {
        self.coverage_status = Some(coverage_status.into());
        self
    }
}

/// Trial-owned pass criteria carried through without scoring it in mabinogion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PassCriteriaEvidence {
    pub owner: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub criteria_id: Option<String>,
    pub summary: String,
    pub machine_conditions: Vec<JsonValue>,
}

impl PassCriteriaEvidence {
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            owner: "mabinogion-trials".to_string(),
            criteria_id: None,
            summary: summary.into(),
            machine_conditions: Vec::new(),
        }
    }

    pub fn with_criteria_id(mut self, criteria_id: impl Into<String>) -> Self {
        self.criteria_id = Some(criteria_id.into());
        self
    }

    pub fn with_machine_condition(mut self, condition: JsonValue) -> Self {
        self.machine_conditions.push(condition);
        self
    }
}

/// Artifact visibility boundary for replay and diagnostic evidence.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactVisibility {
    PublicSummary,
    PrivateRaw,
    InternalOnly,
}

/// Failure replay artifact metadata. The artifact contents stay outside this struct.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FailureReplayArtifact {
    pub artifact_id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    pub visibility: ArtifactVisibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl FailureReplayArtifact {
    pub fn new(
        artifact_id: impl Into<String>,
        kind: impl Into<String>,
        visibility: ArtifactVisibility,
    ) -> Self {
        Self {
            artifact_id: artifact_id.into(),
            kind: kind.into(),
            path: None,
            media_type: None,
            digest: None,
            visibility,
            description: None,
        }
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_media_type(mut self, media_type: impl Into<String>) -> Self {
        self.media_type = Some(media_type.into());
        self
    }

    pub fn with_digest(mut self, digest: impl Into<String>) -> Self {
        self.digest = Some(digest.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    fn public_summary(&self) -> Option<PublicFailureReplayArtifact> {
        if self.visibility != ArtifactVisibility::PublicSummary {
            return None;
        }
        Some(PublicFailureReplayArtifact {
            artifact_id: self.artifact_id.clone(),
            kind: self.kind.clone(),
            media_type: self.media_type.clone(),
            visibility: self.visibility,
            description: self.description.clone(),
        })
    }
}

/// Public-safe failure replay artifact metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublicFailureReplayArtifact {
    pub artifact_id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    pub visibility: ArtifactVisibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Report-friendly metrics exported as evidence, not as Prometheus samples.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunEvidenceMetrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reconnect_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_count: Option<u64>,
    pub recovery_events: Vec<RecoveryEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_usage: Option<ResourceUsageSummary>,
}

impl RunEvidenceMetrics {
    pub fn with_latency_ms(mut self, latency_ms: f64) -> Self {
        self.latency_ms = Some(latency_ms);
        self
    }

    pub fn with_reconnect_count(mut self, reconnect_count: u64) -> Self {
        self.reconnect_count = Some(reconnect_count);
        self
    }

    pub fn with_error_count(mut self, error_count: u64) -> Self {
        self.error_count = Some(error_count);
        self
    }

    pub fn with_recovery_event(mut self, event: RecoveryEvent) -> Self {
        self.recovery_events.push(event);
        self
    }

    pub fn with_resource_usage(mut self, usage: ResourceUsageSummary) -> Self {
        self.resource_usage = Some(usage);
        self
    }
}

/// Recovery event summary suitable for proof/report generation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecoveryEvent {
    pub event_id: String,
    pub occurred_at: DateTime<Utc>,
    pub kind: String,
    pub summary: String,
}

impl RecoveryEvent {
    pub fn new(
        event_id: impl Into<String>,
        kind: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            occurred_at: Utc::now(),
            kind: kind.into(),
            summary: summary.into(),
        }
    }

    pub fn occurred_at(mut self, occurred_at: DateTime<Utc>) -> Self {
        self.occurred_at = occurred_at;
        self
    }
}

/// Resource usage summary captured by a caller-provided runner.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ResourceUsageSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_memory_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_cpu_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_open_file_descriptors: Option<u64>,
}

/// Boundary between public proof summary and private raw diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicPrivateBoundary {
    pub public_summary_fields: Vec<String>,
    pub private_artifact_fields: Vec<String>,
    pub private_artifact_policy: String,
}

impl Default for PublicPrivateBoundary {
    fn default() -> Self {
        Self {
            public_summary_fields: vec![
                "run_id".to_string(),
                "engine_version".to_string(),
                "protocol_profile".to_string(),
                "trial_suite_version".to_string(),
                "started_at".to_string(),
                "ended_at".to_string(),
                "feature_flags".to_string(),
                "pass_criteria".to_string(),
                "failure_replay_artifacts.public_summary".to_string(),
                "metrics".to_string(),
            ],
            private_artifact_fields: vec![
                "failure_replay_artifacts.path".to_string(),
                "failure_replay_artifacts.digest".to_string(),
                "raw_logs".to_string(),
                "packet_captures".to_string(),
            ],
            private_artifact_policy: "private raw artifacts are referenced by metadata and are not embedded in public summaries".to_string(),
        }
    }
}

/// Full run evidence exported by mabinogion for Imugi and Trials consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvidence {
    pub run_evidence_schema_version: String,
    pub trial_artifact_contract_version: String,
    pub runtime_contract_version: String,
    pub snapshot_metadata_version: String,
    pub run_id: String,
    pub engine_version: String,
    pub protocol_profile: ProtocolProfileEvidence,
    pub trial_suite_version: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub feature_flags: Vec<String>,
    pub pass_criteria: PassCriteriaEvidence,
    pub failure_replay_artifacts: Vec<FailureReplayArtifact>,
    pub public_private_boundary: PublicPrivateBoundary,
    pub runtime_snapshot: RuntimeSessionSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<RunEvidenceMetrics>,
}

impl RunEvidence {
    /// Returns a public-safe evidence summary for proof report input.
    pub fn public_summary(&self) -> PublicRunEvidenceSummary {
        PublicRunEvidenceSummary {
            run_evidence_schema_version: self.run_evidence_schema_version.clone(),
            run_id: self.run_id.clone(),
            engine_version: self.engine_version.clone(),
            protocol_profile: self.protocol_profile.clone(),
            trial_suite_version: self.trial_suite_version.clone(),
            started_at: self.started_at,
            ended_at: self.ended_at,
            feature_flags: self.feature_flags.clone(),
            pass_criteria: self.pass_criteria.clone(),
            failure_replay_artifacts: self
                .failure_replay_artifacts
                .iter()
                .filter_map(FailureReplayArtifact::public_summary)
                .collect(),
            public_private_boundary: self.public_private_boundary.clone(),
            metrics: self.metrics.clone(),
        }
    }
}

/// Public-safe proof/report summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublicRunEvidenceSummary {
    pub run_evidence_schema_version: String,
    pub run_id: String,
    pub engine_version: String,
    pub protocol_profile: ProtocolProfileEvidence,
    pub trial_suite_version: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub feature_flags: Vec<String>,
    pub pass_criteria: PassCriteriaEvidence,
    pub failure_replay_artifacts: Vec<PublicFailureReplayArtifact>,
    pub public_private_boundary: PublicPrivateBoundary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<RunEvidenceMetrics>,
}

/// Builder used by Imugi/Trials runners to assemble evidence from runtime output.
#[derive(Debug, Clone)]
pub struct RunEvidenceBuilder {
    evidence: RunEvidence,
}

impl RunEvidenceBuilder {
    pub fn new(
        run_id: impl Into<String>,
        trial_suite_version: impl Into<String>,
        protocol_profile: ProtocolProfileEvidence,
        pass_criteria: PassCriteriaEvidence,
        runtime_snapshot: RuntimeSessionSnapshot,
    ) -> Self {
        let now = Utc::now();
        Self {
            evidence: RunEvidence {
                run_evidence_schema_version: RUN_EVIDENCE_SCHEMA_VERSION.to_string(),
                trial_artifact_contract_version: TRIAL_ARTIFACT_CONTRACT_VERSION.to_string(),
                runtime_contract_version: RUNTIME_CONTRACT_VERSION.to_string(),
                snapshot_metadata_version: SNAPSHOT_METADATA_VERSION.to_string(),
                run_id: run_id.into(),
                engine_version: mabi_core::RELEASE_VERSION.to_string(),
                protocol_profile,
                trial_suite_version: trial_suite_version.into(),
                started_at: now,
                ended_at: now,
                feature_flags: Vec::new(),
                pass_criteria,
                failure_replay_artifacts: Vec::new(),
                public_private_boundary: PublicPrivateBoundary::default(),
                runtime_snapshot,
                metrics: None,
            },
        }
    }

    pub fn engine_version(mut self, engine_version: impl Into<String>) -> Self {
        self.evidence.engine_version = engine_version.into();
        self
    }

    pub fn started_at(mut self, started_at: DateTime<Utc>) -> Self {
        self.evidence.started_at = started_at;
        self
    }

    pub fn ended_at(mut self, ended_at: DateTime<Utc>) -> Self {
        self.evidence.ended_at = ended_at;
        self
    }

    pub fn feature_flags(mut self, feature_flags: Vec<String>) -> Self {
        self.evidence.feature_flags = feature_flags;
        self
    }

    pub fn add_feature_flag(mut self, feature_flag: impl Into<String>) -> Self {
        self.evidence.feature_flags.push(feature_flag.into());
        self
    }

    pub fn add_failure_replay_artifact(mut self, artifact: FailureReplayArtifact) -> Self {
        self.evidence.failure_replay_artifacts.push(artifact);
        self
    }

    pub fn metrics(mut self, metrics: RunEvidenceMetrics) -> Self {
        self.evidence.metrics = Some(metrics);
        self
    }

    pub fn public_private_boundary(mut self, boundary: PublicPrivateBoundary) -> Self {
        self.evidence.public_private_boundary = boundary;
        self
    }

    pub fn build(self) -> RunEvidence {
        self.evidence
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value as JsonValue};

    use crate::evidence::{
        ArtifactVisibility, FailureReplayArtifact, PassCriteriaEvidence, ProtocolProfileEvidence,
        PublicPrivateBoundary, RecoveryEvent, ResourceUsageSummary, RunEvidenceBuilder,
        RunEvidenceMetrics, RUN_EVIDENCE_SCHEMA_VERSION, TRIAL_ARTIFACT_CONTRACT_VERSION,
    };
    use crate::service::{ServiceSnapshot, ServiceState, RUNTIME_CONTRACT_VERSION};
    use crate::session::RuntimeSessionSnapshot;

    fn snapshot() -> RuntimeSessionSnapshot {
        let mut service = ServiceSnapshot::new("evidence-modbus");
        service.status.state = ServiceState::Running;
        service.status.ready = true;
        service.ensure_runtime_metadata();
        RuntimeSessionSnapshot::new(vec![service])
    }

    fn evidence() -> crate::evidence::RunEvidence {
        RunEvidenceBuilder::new(
            "run-001",
            "trials-2026.05",
            ProtocolProfileEvidence::new("modbus", "modbus.l1.function_code")
                .with_capability_id("modbus.function_code")
                .with_lane("deterministic"),
            PassCriteriaEvidence::new("All required Modbus function code checks pass")
                .with_criteria_id("modbus-l1-pass")
                .with_machine_condition(json!({"kind": "all_required_checks_pass"})),
            snapshot(),
        )
        .engine_version("1.2.3")
        .feature_flags(vec!["opcua-https-disabled".to_string()])
        .add_failure_replay_artifact(
            FailureReplayArtifact::new(
                "public-summary",
                "failure_summary",
                ArtifactVisibility::PublicSummary,
            )
            .with_media_type("application/json")
            .with_description("Public replay summary"),
        )
        .add_failure_replay_artifact(
            FailureReplayArtifact::new("raw-log", "raw_log", ArtifactVisibility::PrivateRaw)
                .with_path("/private/raw.log")
                .with_digest("sha256:abc123")
                .with_media_type("text/plain"),
        )
        .metrics(
            RunEvidenceMetrics::default()
                .with_latency_ms(12.5)
                .with_reconnect_count(1)
                .with_error_count(0)
                .with_recovery_event(RecoveryEvent::new(
                    "recovery-001",
                    "reconnect",
                    "Client reconnected after injected disconnect",
                ))
                .with_resource_usage(ResourceUsageSummary {
                    peak_memory_bytes: Some(2048),
                    average_cpu_percent: Some(1.5),
                    max_open_file_descriptors: None,
                }),
        )
        .public_private_boundary(PublicPrivateBoundary::default())
        .build()
    }

    #[test]
    fn run_evidence_serializes_required_contract_fields() {
        let evidence = evidence();
        let value = serde_json::to_value(&evidence).expect("evidence serializes");

        for field in [
            "run_id",
            "engine_version",
            "protocol_profile",
            "trial_suite_version",
            "started_at",
            "ended_at",
            "feature_flags",
            "pass_criteria",
            "failure_replay_artifacts",
            "public_private_boundary",
        ] {
            assert!(value.get(field).is_some(), "{field} should serialize");
        }
        assert_eq!(
            value["run_evidence_schema_version"],
            RUN_EVIDENCE_SCHEMA_VERSION
        );
        assert_eq!(
            value["trial_artifact_contract_version"],
            TRIAL_ARTIFACT_CONTRACT_VERSION
        );
        assert_eq!(value["runtime_contract_version"], RUNTIME_CONTRACT_VERSION);
        assert!(value.get("scoring_result").is_none());
    }

    #[test]
    fn run_evidence_builder_preserves_runtime_snapshot_and_metadata() {
        let evidence = evidence();

        assert_eq!(evidence.run_id, "run-001");
        assert_eq!(evidence.trial_suite_version, "trials-2026.05");
        assert_eq!(evidence.protocol_profile.protocol, "modbus");
        assert_eq!(evidence.runtime_snapshot.services.len(), 1);
        assert!(evidence.runtime_snapshot.services[0]
            .runtime_metadata()
            .is_some());
        assert_eq!(evidence.failure_replay_artifacts.len(), 2);
        assert_eq!(
            evidence.metrics.as_ref().and_then(|m| m.error_count),
            Some(0)
        );
    }

    #[test]
    fn public_summary_excludes_private_artifact_paths_and_raw_fields() {
        let summary = evidence().public_summary();
        let value = serde_json::to_value(&summary).expect("summary serializes");
        let text = serde_json::to_string(&value).expect("summary stringifies");

        assert_eq!(summary.failure_replay_artifacts.len(), 1);
        assert!(!text.contains("/private/raw.log"));
        assert!(!text.contains("sha256:abc123"));
        assert!(text.contains("public-summary"));
        assert!(value.get("runtime_snapshot").is_none());
    }

    #[test]
    fn failure_replay_artifacts_support_public_and_private_visibility() {
        let evidence = evidence();
        let visibilities = evidence
            .failure_replay_artifacts
            .iter()
            .map(|artifact| artifact.visibility)
            .collect::<Vec<_>>();

        assert!(visibilities.contains(&ArtifactVisibility::PublicSummary));
        assert!(visibilities.contains(&ArtifactVisibility::PrivateRaw));
        let value: JsonValue = serde_json::to_value(&evidence).expect("evidence serializes");
        assert_eq!(
            value["failure_replay_artifacts"][1]["visibility"],
            "private_raw"
        );
    }
}
