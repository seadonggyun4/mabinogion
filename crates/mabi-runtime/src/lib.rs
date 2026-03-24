//! Shared runtime contracts for the Mabinogion workspace.

pub mod device;
pub mod driver;
pub mod service;
pub mod session;

pub use device::{CoreDevicePort, DevicePort, DeviceRegistry, DynDevicePort};
pub use driver::{
    ProtocolCatalogEntry, ProtocolDescriptor, ProtocolDriver, ProtocolDriverRegistry,
    ProtocolLaunchSpec,
};
pub use service::{
    ManagedService, RuntimeError, RuntimeResult, ServiceContext, ServiceEvent, ServiceHandle,
    ServiceSnapshot, ServiceState, ServiceStatus,
};
pub use session::{DevicePortLayer, RuntimeExtensions, RuntimeSession, RuntimeSessionSpec};
