//! Shared runtime contracts for the Mabinogion workspace.

pub mod device;
pub mod driver;
pub mod evidence;
pub mod service;
pub mod session;

pub use device::{CoreDevicePort, DevicePort, DeviceRegistry, DynDevicePort};
pub use driver::{
    ProtocolCatalogEntry, ProtocolDescriptor, ProtocolDriver, ProtocolDriverRegistry,
    ProtocolLaunchSpec,
};
pub use evidence::{
    ArtifactVisibility, FailureReplayArtifact, PassCriteriaEvidence, ProtocolProfileEvidence,
    PublicFailureReplayArtifact, PublicPrivateBoundary, PublicRunEvidenceSummary, RecoveryEvent,
    ResourceUsageSummary, RunEvidence, RunEvidenceBuilder, RunEvidenceMetrics,
    RUN_EVIDENCE_SCHEMA_VERSION, TRIAL_ARTIFACT_CONTRACT_VERSION,
};
pub use service::{
    ManagedService, RuntimeError, RuntimeErrorInfo, RuntimeErrorKind, RuntimeResult,
    ServiceContext, ServiceEvent, ServiceHandle, ServiceReadinessReport, ServiceRuntimeMetadata,
    ServiceSnapshot, ServiceState, ServiceStatus, RUNTIME_CONTRACT_VERSION, RUNTIME_METADATA_KEY,
    SNAPSHOT_METADATA_VERSION,
};
pub use session::{
    DevicePortLayer, RuntimeExtensions, RuntimeSession, RuntimeSessionSnapshot, RuntimeSessionSpec,
};
