//! Shared datastore and server context abstractions.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::error::ModbusResult;
use crate::register::{RegisterStore, RegisterType};
use crate::registers::SparseRegisterStore;
use crate::types::WordOrder;

/// Shared trait implemented by dense and sparse Modbus register stores.
pub trait AddressSpace: Send + Sync {
    fn count(&self, reg_type: RegisterType) -> u16;
    fn read_coils(&self, address: u16, quantity: u16) -> ModbusResult<Vec<bool>>;
    fn write_coil(&self, address: u16, value: bool) -> ModbusResult<()>;
    fn write_coils(&self, address: u16, values: &[bool]) -> ModbusResult<()>;
    fn read_discrete_inputs(&self, address: u16, quantity: u16) -> ModbusResult<Vec<bool>>;
    fn set_discrete_input(&self, address: u16, value: bool) -> ModbusResult<()>;
    fn read_holding_registers(&self, address: u16, quantity: u16) -> ModbusResult<Vec<u16>>;
    fn write_holding_register(&self, address: u16, value: u16) -> ModbusResult<()>;
    fn write_holding_registers(&self, address: u16, values: &[u16]) -> ModbusResult<()>;
    fn read_input_registers(&self, address: u16, quantity: u16) -> ModbusResult<Vec<u16>>;
    fn set_input_register(&self, address: u16, value: u16) -> ModbusResult<()>;
    fn set_input_registers(&self, address: u16, values: &[u16]) -> ModbusResult<()>;

    fn mask_write_holding_register(
        &self,
        address: u16,
        and_mask: u16,
        or_mask: u16,
    ) -> ModbusResult<()> {
        let current = self.read_holding_registers(address, 1)?[0];
        let updated = (current & and_mask) | (or_mask & !and_mask);
        self.write_holding_register(address, updated)
    }
}

/// Shared trait-object register space.
pub type SharedAddressSpace = Arc<dyn AddressSpace>;

/// Canonical broadcast handling policy for the Modbus canonical stack.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BroadcastPolicy {
    /// Broadcast writes are fanned out to all registered units.
    WriteAll,

    /// Broadcast handling is disabled.
    Disabled,

    /// Broadcast writes are sent only to the selected unit IDs.
    #[serde(skip)]
    SelectiveList(Vec<u8>),

    /// Broadcast writes are routed to one designated unit.
    EchoToUnit(u8),
}

impl Default for BroadcastPolicy {
    fn default() -> Self {
        Self::WriteAll
    }
}

impl fmt::Display for BroadcastPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WriteAll => write!(f, "Write to all units"),
            Self::Disabled => write!(f, "Disabled"),
            Self::SelectiveList(units) => write!(f, "Selective ({} units)", units.len()),
            Self::EchoToUnit(id) => write!(f, "Echo to unit {}", id),
        }
    }
}

/// Dense register store alias for the new context-oriented surface.
pub type DenseRegisterStore = RegisterStore;

impl AddressSpace for RegisterStore {
    fn count(&self, reg_type: RegisterType) -> u16 {
        self.count(reg_type)
    }

    fn read_coils(&self, address: u16, quantity: u16) -> ModbusResult<Vec<bool>> {
        self.read_coils(address, quantity)
    }

    fn write_coil(&self, address: u16, value: bool) -> ModbusResult<()> {
        self.write_coil(address, value)
    }

    fn write_coils(&self, address: u16, values: &[bool]) -> ModbusResult<()> {
        self.write_coils(address, values)
    }

    fn read_discrete_inputs(&self, address: u16, quantity: u16) -> ModbusResult<Vec<bool>> {
        self.read_discrete_inputs(address, quantity)
    }

    fn set_discrete_input(&self, address: u16, value: bool) -> ModbusResult<()> {
        self.set_discrete_input(address, value)
    }

    fn read_holding_registers(&self, address: u16, quantity: u16) -> ModbusResult<Vec<u16>> {
        self.read_holding_registers(address, quantity)
    }

    fn write_holding_register(&self, address: u16, value: u16) -> ModbusResult<()> {
        self.write_holding_register(address, value)
    }

    fn write_holding_registers(&self, address: u16, values: &[u16]) -> ModbusResult<()> {
        self.write_holding_registers(address, values)
    }

    fn read_input_registers(&self, address: u16, quantity: u16) -> ModbusResult<Vec<u16>> {
        self.read_input_registers(address, quantity)
    }

    fn set_input_register(&self, address: u16, value: u16) -> ModbusResult<()> {
        self.set_input_register(address, value)
    }

    fn set_input_registers(&self, address: u16, values: &[u16]) -> ModbusResult<()> {
        self.set_input_registers(address, values)
    }

    fn mask_write_holding_register(
        &self,
        address: u16,
        and_mask: u16,
        or_mask: u16,
    ) -> ModbusResult<()> {
        self.mask_write_holding_register(address, and_mask, or_mask)
            .map(|_| ())
    }
}

impl AddressSpace for SparseRegisterStore {
    fn count(&self, reg_type: RegisterType) -> u16 {
        let range = self.config().get_range(sparse_register_type(reg_type));
        range.end.saturating_sub(range.start).saturating_add(1)
    }

    fn read_coils(&self, address: u16, quantity: u16) -> ModbusResult<Vec<bool>> {
        self.read_coils(address, quantity)
    }

    fn write_coil(&self, address: u16, value: bool) -> ModbusResult<()> {
        self.write_coil(address, value)
    }

    fn write_coils(&self, address: u16, values: &[bool]) -> ModbusResult<()> {
        self.write_coils(address, values)
    }

    fn read_discrete_inputs(&self, address: u16, quantity: u16) -> ModbusResult<Vec<bool>> {
        self.read_discrete_inputs(address, quantity)
    }

    fn set_discrete_input(&self, address: u16, value: bool) -> ModbusResult<()> {
        self.set_discrete_input(address, value)
    }

    fn read_holding_registers(&self, address: u16, quantity: u16) -> ModbusResult<Vec<u16>> {
        self.read_holding_registers(address, quantity)
    }

    fn write_holding_register(&self, address: u16, value: u16) -> ModbusResult<()> {
        self.write_holding_register(address, value)
    }

    fn write_holding_registers(&self, address: u16, values: &[u16]) -> ModbusResult<()> {
        self.write_holding_registers(address, values)
    }

    fn read_input_registers(&self, address: u16, quantity: u16) -> ModbusResult<Vec<u16>> {
        self.read_input_registers(address, quantity)
    }

    fn set_input_register(&self, address: u16, value: u16) -> ModbusResult<()> {
        self.set_input_register(address, value)
    }

    fn set_input_registers(&self, address: u16, values: &[u16]) -> ModbusResult<()> {
        self.set_input_registers(address, values)
    }
}

fn sparse_register_type(reg_type: RegisterType) -> crate::registers::RegisterType {
    match reg_type {
        RegisterType::Coil => crate::registers::RegisterType::Coil,
        RegisterType::DiscreteInput => crate::registers::RegisterType::DiscreteInput,
        RegisterType::HoldingRegister => crate::registers::RegisterType::HoldingRegister,
        RegisterType::InputRegister => crate::registers::RegisterType::InputRegister,
    }
}

/// Per-unit Modbus device context.
#[derive(Clone)]
pub struct DeviceContext {
    unit_id: u8,
    name: Arc<str>,
    address_space: SharedAddressSpace,
    word_order: WordOrder,
    response_delay: Duration,
    broadcast_enabled: bool,
}

impl DeviceContext {
    pub fn new(
        unit_id: u8,
        name: impl Into<String>,
        address_space: SharedAddressSpace,
        word_order: WordOrder,
    ) -> Self {
        Self {
            unit_id,
            name: Arc::from(name.into()),
            address_space,
            word_order,
            response_delay: Duration::ZERO,
            broadcast_enabled: true,
        }
    }

    pub fn with_response_delay(mut self, delay: Duration) -> Self {
        self.response_delay = delay;
        self
    }

    pub fn with_broadcast(mut self, enabled: bool) -> Self {
        self.broadcast_enabled = enabled;
        self
    }

    pub fn unit_id(&self) -> u8 {
        self.unit_id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn address_space(&self) -> SharedAddressSpace {
        self.address_space.clone()
    }

    pub fn word_order(&self) -> WordOrder {
        self.word_order
    }

    pub fn response_delay(&self) -> Duration {
        self.response_delay
    }

    pub fn broadcast_enabled(&self) -> bool {
        self.broadcast_enabled
    }
}

impl std::fmt::Debug for DeviceContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceContext")
            .field("unit_id", &self.unit_id)
            .field("name", &self.name())
            .field("word_order", &self.word_order)
            .field("response_delay", &self.response_delay)
            .field("broadcast_enabled", &self.broadcast_enabled)
            .finish()
    }
}

/// Request execution target resolved from the server context.
#[derive(Clone)]
pub struct RequestTarget {
    unit_id: u8,
    device: Option<Arc<DeviceContext>>,
    address_space: SharedAddressSpace,
    word_order: WordOrder,
}

impl RequestTarget {
    pub fn unit_id(&self) -> u8 {
        self.unit_id
    }

    pub fn device(&self) -> Option<Arc<DeviceContext>> {
        self.device.clone()
    }

    pub fn address_space(&self) -> SharedAddressSpace {
        self.address_space.clone()
    }

    pub fn word_order(&self) -> WordOrder {
        self.word_order
    }
}

/// Shared server context containing unit registry and fallback datastore.
pub struct ServerContext {
    units: DashMap<u8, Arc<DeviceContext>>,
    default_space: RwLock<SharedAddressSpace>,
    broadcast_policy: RwLock<BroadcastPolicy>,
}

impl ServerContext {
    pub fn new(default_space: SharedAddressSpace) -> Self {
        Self {
            units: DashMap::new(),
            default_space: RwLock::new(default_space),
            broadcast_policy: RwLock::new(BroadcastPolicy::WriteAll),
        }
    }

    pub fn register(&self, device: Arc<DeviceContext>) -> Option<Arc<DeviceContext>> {
        self.units.insert(device.unit_id(), device)
    }

    pub fn remove(&self, unit_id: u8) -> Option<Arc<DeviceContext>> {
        self.units.remove(&unit_id).map(|(_, device)| device)
    }

    pub fn device(&self, unit_id: u8) -> Option<Arc<DeviceContext>> {
        self.units.get(&unit_id).map(|entry| entry.value().clone())
    }

    pub fn devices(&self) -> Vec<Arc<DeviceContext>> {
        self.units
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn set_default_space(&self, default_space: SharedAddressSpace) {
        *self.default_space.write() = default_space;
    }

    pub fn default_space(&self) -> SharedAddressSpace {
        self.default_space.read().clone()
    }

    pub fn set_broadcast_enabled(&self, enabled: bool) {
        let policy = if enabled {
            BroadcastPolicy::WriteAll
        } else {
            BroadcastPolicy::Disabled
        };
        self.set_broadcast_policy(policy);
    }

    pub fn broadcast_enabled(&self) -> bool {
        !matches!(self.broadcast_policy(), BroadcastPolicy::Disabled)
    }

    pub fn set_broadcast_policy(&self, policy: BroadcastPolicy) {
        *self.broadcast_policy.write() = policy;
    }

    pub fn broadcast_policy(&self) -> BroadcastPolicy {
        self.broadcast_policy.read().clone()
    }

    pub fn target_for_unit(&self, unit_id: u8) -> Option<RequestTarget> {
        if unit_id == 0 {
            return None;
        }

        if let Some(device) = self.device(unit_id) {
            return Some(RequestTarget {
                unit_id,
                word_order: device.word_order(),
                address_space: device.address_space(),
                device: Some(device),
            });
        }

        None
    }

    pub fn broadcast_targets(&self) -> Vec<RequestTarget> {
        match self.broadcast_policy() {
            BroadcastPolicy::WriteAll => self
                .devices()
                .into_iter()
                .filter(|device| device.broadcast_enabled())
                .map(|device| RequestTarget {
                    unit_id: device.unit_id(),
                    word_order: device.word_order(),
                    address_space: device.address_space(),
                    device: Some(device),
                })
                .collect(),
            BroadcastPolicy::Disabled => Vec::new(),
            BroadcastPolicy::SelectiveList(unit_ids) => unit_ids
                .into_iter()
                .filter_map(|unit_id| self.target_for_unit(unit_id))
                .filter(|target| {
                    target
                        .device()
                        .map(|device| device.broadcast_enabled())
                        .unwrap_or(false)
                })
                .collect(),
            BroadcastPolicy::EchoToUnit(unit_id) => self
                .target_for_unit(unit_id)
                .filter(|target| {
                    target
                        .device()
                        .map(|device| device.broadcast_enabled())
                        .unwrap_or(false)
                })
                .into_iter()
                .collect(),
        }
    }

    pub fn fallback_target(&self, unit_id: u8) -> RequestTarget {
        RequestTarget {
            unit_id,
            device: None,
            address_space: self.default_space(),
            word_order: WordOrder::default(),
        }
    }
}

impl std::fmt::Debug for ServerContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerContext")
            .field("units", &self.units.len())
            .field("broadcast_policy", &self.broadcast_policy())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{AddressSpace, BroadcastPolicy, DeviceContext, ServerContext};
    use crate::register::{RegisterStore, RegisterType};
    use crate::registers::{RegisterStoreConfig, SparseRegisterStore};
    use crate::types::WordOrder;

    fn assert_address_space_contract(space: &dyn AddressSpace) {
        assert_eq!(space.count(RegisterType::HoldingRegister), 100);

        space.write_coil(1, true).unwrap();
        assert_eq!(space.read_coils(1, 1).unwrap(), vec![true]);

        space.write_holding_register(2, 0x1234).unwrap();
        assert_eq!(space.read_holding_registers(2, 1).unwrap(), vec![0x1234]);

        space
            .mask_write_holding_register(2, 0xFF00, 0x0056)
            .unwrap();
        assert_eq!(space.read_holding_registers(2, 1).unwrap(), vec![0x1256]);

        space.set_input_register(3, 0xABCD).unwrap();
        assert_eq!(space.read_input_registers(3, 1).unwrap(), vec![0xABCD]);

        space.set_discrete_input(4, true).unwrap();
        assert_eq!(space.read_discrete_inputs(4, 1).unwrap(), vec![true]);
    }

    #[test]
    fn dense_and_sparse_address_spaces_follow_the_same_contract() {
        let dense = RegisterStore::new(100, 100, 100, 100);
        let sparse = SparseRegisterStore::new(RegisterStoreConfig::minimal());

        assert_address_space_contract(&dense);
        assert_address_space_contract(&sparse);
    }

    #[test]
    fn server_context_routes_units_and_broadcast_targets() {
        let default_space = Arc::new(RegisterStore::new(16, 16, 16, 16));
        default_space.write_holding_register(1, 0xCAFE).unwrap();

        let server = ServerContext::new(default_space.clone());

        let unit_space = Arc::new(RegisterStore::new(16, 16, 16, 16));
        unit_space.write_holding_register(1, 0xBEEF).unwrap();

        let device = Arc::new(
            DeviceContext::new(7, "Pump-A", unit_space.clone(), WordOrder::default())
                .with_broadcast(false),
        );
        server.register(device.clone());

        let unit_target = server.target_for_unit(7).unwrap();
        assert_eq!(unit_target.unit_id(), 7);
        assert_eq!(
            unit_target
                .address_space()
                .read_holding_registers(1, 1)
                .unwrap(),
            vec![0xBEEF]
        );
        assert_eq!(unit_target.device().unwrap().name(), "Pump-A");

        assert!(server.target_for_unit(0).is_none());
        assert!(server.broadcast_targets().is_empty());

        server.set_broadcast_enabled(false);
        assert!(server.broadcast_targets().is_empty());
    }

    #[test]
    fn server_context_filters_broadcast_targets_by_policy_and_unit_opt_out() {
        let server = ServerContext::new(Arc::new(RegisterStore::new(16, 16, 16, 16)));

        let unit_1 = Arc::new(DeviceContext::new(
            1,
            "Unit 1",
            Arc::new(RegisterStore::new(16, 16, 16, 16)),
            WordOrder::default(),
        ));
        let unit_2 = Arc::new(
            DeviceContext::new(
                2,
                "Unit 2",
                Arc::new(RegisterStore::new(16, 16, 16, 16)),
                WordOrder::default(),
            )
            .with_broadcast(false),
        );
        let unit_3 = Arc::new(DeviceContext::new(
            3,
            "Unit 3",
            Arc::new(RegisterStore::new(16, 16, 16, 16)),
            WordOrder::default(),
        ));

        server.register(unit_1);
        server.register(unit_2);
        server.register(unit_3);

        let mut all_targets = server
            .broadcast_targets()
            .into_iter()
            .map(|target| target.unit_id())
            .collect::<Vec<_>>();
        all_targets.sort_unstable();
        assert_eq!(all_targets, vec![1, 3]);

        server.set_broadcast_policy(BroadcastPolicy::SelectiveList(vec![2, 3]));
        let selective_targets = server
            .broadcast_targets()
            .into_iter()
            .map(|target| target.unit_id())
            .collect::<Vec<_>>();
        assert_eq!(selective_targets, vec![3]);

        server.set_broadcast_policy(BroadcastPolicy::EchoToUnit(1));
        let echo_targets = server
            .broadcast_targets()
            .into_iter()
            .map(|target| target.unit_id())
            .collect::<Vec<_>>();
        assert_eq!(echo_targets, vec![1]);
    }
}
