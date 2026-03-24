use mabi_runtime::{ProtocolCatalogEntry, ProtocolDriverRegistry};

/// Builds the workspace protocol driver registry from crate-owned drivers.
pub fn workspace_protocol_registry() -> ProtocolDriverRegistry {
    let mut registry = ProtocolDriverRegistry::new();
    registry.register(mabi_modbus::driver());
    registry.register(mabi_opcua::driver());
    registry.register(mabi_bacnet::driver());
    registry.register(mabi_knx::driver());
    registry
}

/// Returns the stable protocol catalog for CLI inspection surfaces.
pub fn protocol_catalog() -> Vec<ProtocolCatalogEntry> {
    workspace_protocol_registry().catalog()
}
