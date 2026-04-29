use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct VerificationContract {
    pub baseline: ContractBaseline,
    pub policies: ContractPolicies,
    pub capabilities: Vec<ContractCapability>,
    pub profiles: Vec<ContractProfile>,
    pub peers: Vec<ContractPeer>,
}

#[derive(Debug, Deserialize)]
pub struct ContractBaseline {
    #[serde(rename = "crate")]
    pub crate_: Option<String>,
    pub protocol_scope: String,
    pub integration_layer_present: bool,
    pub interop_plane_present: bool,
    #[serde(default)]
    pub capture_corpus_present: bool,
    #[serde(default)]
    pub perf_contract_present: bool,
    pub default_workspace_command: String,
}

#[derive(Debug, Deserialize)]
pub struct ContractPolicies {
    pub default_workspace_lane: String,
    pub interop_lane: String,
    pub perf_lane: String,
    #[serde(default)]
    pub capture_lane: String,
    #[serde(default)]
    pub default_perf_thresholds_forbidden: bool,
    pub gui_automation: String,
    pub production_dependency_policy: String,
}

#[derive(Debug, Deserialize)]
pub struct ContractCapability {
    pub id: String,
    pub description: String,
    pub implemented_in_core: bool,
    pub unit_coverage: String,
    pub integration_coverage: String,
    pub interop_coverage: String,
    #[serde(default)]
    pub capture_coverage: String,
    pub code_refs: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ContractProfile {
    pub id: String,
    pub lane: String,
    pub capabilities: Vec<String>,
    pub phase_introduced: String,
    pub notes: String,
}

#[derive(Debug, Deserialize)]
pub struct ContractPeer {
    pub id: String,
    pub automation_lane: String,
    pub role: String,
    pub primary_capabilities: Vec<String>,
    pub secondary_capabilities: Vec<String>,
    pub excluded_from_current_ci: bool,
    pub notes: String,
}

static CONTRACT: OnceLock<VerificationContract> = OnceLock::new();

fn load_contract() -> VerificationContract {
    serde_yaml::from_str(include_str!(
        "../../../../docs/bacnet-simulator/verification-contract.yaml"
    ))
    .expect("verification contract should parse")
}

pub fn contract() -> &'static VerificationContract {
    CONTRACT.get_or_init(load_contract)
}

pub fn profile(id: &str) -> &'static ContractProfile {
    contract()
        .profiles
        .iter()
        .find(|profile| profile.id == id)
        .unwrap_or_else(|| panic!("verification contract missing profile {id}"))
}

pub fn capability(id: &str) -> &'static ContractCapability {
    contract()
        .capabilities
        .iter()
        .find(|capability| capability.id == id)
        .unwrap_or_else(|| panic!("verification contract missing capability {id}"))
}

pub fn peer(id: &str) -> &'static ContractPeer {
    contract()
        .peers
        .iter()
        .find(|peer| peer.id == id)
        .unwrap_or_else(|| panic!("verification contract missing peer {id}"))
}
