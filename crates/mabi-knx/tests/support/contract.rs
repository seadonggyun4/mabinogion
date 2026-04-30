use serde::Deserialize;

use super::TestResult;

const CONTRACT: &str = include_str!("../../../../docs/knx-simulator/verification-contract.yaml");

#[derive(Debug, Deserialize)]
struct VerificationContract {
    policies: Policies,
    capabilities: Vec<Capability>,
    profiles: Vec<Profile>,
    peers: Vec<Peer>,
}

#[derive(Debug, Deserialize)]
struct Policies {
    default_workspace_lane: String,
    interop_lane: String,
    #[serde(default)]
    interop_execution_model: String,
}

#[derive(Debug, Deserialize)]
struct Capability {
    id: String,
    interop_coverage: String,
}

#[derive(Debug, Deserialize)]
struct Profile {
    id: String,
    lane: String,
}

#[derive(Debug, Deserialize)]
struct Peer {
    id: String,
    automation_lane: String,
}

fn load_contract() -> TestResult<VerificationContract> {
    Ok(serde_yaml::from_str(CONTRACT)?)
}

pub fn assert_policy(expected_default_lane: &str, expected_interop_lane: &str) -> TestResult {
    let contract = load_contract()?;
    if contract.policies.default_workspace_lane != expected_default_lane {
        return Err(format!(
            "default workspace lane expected `{expected_default_lane}`, found `{}`",
            contract.policies.default_workspace_lane
        )
        .into());
    }
    if contract.policies.interop_lane != expected_interop_lane {
        return Err(format!(
            "interop lane expected `{expected_interop_lane}`, found `{}`",
            contract.policies.interop_lane
        )
        .into());
    }
    if contract.policies.interop_execution_model != "repo_owned_container_matrix" {
        return Err(format!(
            "interop execution model expected `repo_owned_container_matrix`, found `{}`",
            contract.policies.interop_execution_model
        )
        .into());
    }
    Ok(())
}

pub fn assert_profile_lane(profile_id: &str, expected_lane: &str) -> TestResult {
    let contract = load_contract()?;
    let profile = contract
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| {
            format!("profile `{profile_id}` is missing from KNX verification contract")
        })?;

    if profile.lane != expected_lane {
        return Err(format!(
            "profile `{profile_id}` expected lane `{expected_lane}`, found `{}`",
            profile.lane
        )
        .into());
    }

    Ok(())
}

pub fn assert_capability_interop_coverage(
    capability_id: &str,
    expected_coverage: &str,
) -> TestResult {
    let contract = load_contract()?;
    let capability = contract
        .capabilities
        .iter()
        .find(|capability| capability.id == capability_id)
        .ok_or_else(|| {
            format!("capability `{capability_id}` is missing from KNX verification contract")
        })?;

    if capability.interop_coverage != expected_coverage {
        return Err(format!(
            "capability `{capability_id}` expected interop coverage `{expected_coverage}`, found `{}`",
            capability.interop_coverage
        )
        .into());
    }

    Ok(())
}

pub fn assert_peer_lane(peer_id: &str, expected_lane: &str) -> TestResult {
    let contract = load_contract()?;
    let peer = contract
        .peers
        .iter()
        .find(|peer| peer.id == peer_id)
        .ok_or_else(|| format!("peer `{peer_id}` is missing from KNX verification contract"))?;

    if peer.automation_lane != expected_lane {
        return Err(format!(
            "peer `{peer_id}` expected automation lane `{expected_lane}`, found `{}`",
            peer.automation_lane
        )
        .into());
    }

    Ok(())
}
