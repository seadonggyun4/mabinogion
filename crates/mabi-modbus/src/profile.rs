//! Profile-driven simulator configuration.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use mabi_core::tags::Tags;
use mabi_core::types::{DataType, ModbusRegisterType};

use crate::context::{DenseRegisterStore, SharedAddressSpace};
use crate::error::ModbusResult;
use crate::registers::RegisterStoreConfig;
use crate::types::WordOrder;

/// Selects the backing datastore for a unit profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DatastoreKind {
    Dense {
        coils: u16,
        discrete_inputs: u16,
        holding_registers: u16,
        input_registers: u16,
    },
    Sparse {
        #[serde(default)]
        config: RegisterStoreConfig,
    },
}

impl Default for DatastoreKind {
    fn default() -> Self {
        Self::Dense {
            coils: 10_000,
            discrete_inputs: 10_000,
            holding_registers: 10_000,
            input_registers: 10_000,
        }
    }
}

impl DatastoreKind {
    pub fn dense_from_counts(
        coils: u16,
        discrete_inputs: u16,
        holding_registers: u16,
        input_registers: u16,
    ) -> Self {
        Self::Dense {
            coils,
            discrete_inputs,
            holding_registers,
            input_registers,
        }
    }

    pub fn build_address_space(&self) -> SharedAddressSpace {
        match self {
            Self::Dense {
                coils,
                discrete_inputs,
                holding_registers,
                input_registers,
            } => Arc::new(DenseRegisterStore::new(
                *coils,
                *discrete_inputs,
                *holding_registers,
                *input_registers,
            )),
            Self::Sparse { config } => {
                Arc::new(crate::registers::SparseRegisterStore::new(config.clone()))
            }
        }
    }
}

/// A single profile-defined point exposed by a virtual unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointProfile {
    pub id: String,
    pub name: String,
    pub register_type: ModbusRegisterType,
    pub address: u16,
    pub data_type: DataType,
}

impl PointProfile {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        register_type: ModbusRegisterType,
        address: u16,
        data_type: DataType,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            register_type,
            address,
            data_type,
        }
    }
}

/// Configuration for a single simulated Modbus unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitProfile {
    pub unit_id: u8,
    pub name: String,
    #[serde(default)]
    pub datastore: DatastoreKind,
    #[serde(default)]
    pub points: Vec<PointProfile>,
    #[serde(default)]
    pub response_delay_ms: u64,
    #[serde(default)]
    pub word_order: WordOrder,
    #[serde(default = "default_true")]
    pub broadcast_enabled: bool,
    #[serde(default, skip_serializing_if = "Tags::is_empty")]
    pub tags: Tags,
}

impl UnitProfile {
    pub fn new(unit_id: u8, name: impl Into<String>) -> Self {
        Self {
            unit_id,
            name: name.into(),
            datastore: DatastoreKind::default(),
            points: Vec::new(),
            response_delay_ms: 0,
            word_order: WordOrder::default(),
            broadcast_enabled: true,
            tags: Tags::new(),
        }
    }

    pub fn with_datastore(mut self, datastore: DatastoreKind) -> Self {
        self.datastore = datastore;
        self
    }

    pub fn with_point(mut self, point: PointProfile) -> Self {
        self.points.push(point);
        self
    }
}

/// Top-level simulator profile.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SimulatorProfile {
    #[serde(default = "default_true")]
    pub broadcast_enabled: bool,
    #[serde(default)]
    pub units: Vec<UnitProfile>,
}

impl SimulatorProfile {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_unit(mut self, unit: UnitProfile) -> Self {
        self.units.push(unit);
        self
    }

    pub fn generated(devices: usize, points_per_device: usize) -> Self {
        GeneratedProfilePreset::new(devices, points_per_device).build()
    }
}

/// Convenience preset for the legacy numeric launch surface.
#[derive(Debug, Clone, Copy)]
pub struct GeneratedProfilePreset {
    devices: usize,
    points_per_device: usize,
}

impl GeneratedProfilePreset {
    pub fn new(devices: usize, points_per_device: usize) -> Self {
        Self {
            devices,
            points_per_device,
        }
    }

    pub fn build(self) -> SimulatorProfile {
        let mut profile = SimulatorProfile::new();
        let family_points = std::cmp::max(1, self.points_per_device / 4) as u16;

        for index in 0..self.devices {
            let unit_id = (index + 1) as u8;
            let datastore = DatastoreKind::dense_from_counts(
                family_points,
                family_points,
                family_points,
                family_points,
            );

            let mut unit =
                UnitProfile::new(unit_id, format!("Device-{}", unit_id)).with_datastore(datastore);

            for point_index in 0..family_points {
                unit = unit
                    .with_point(PointProfile::new(
                        format!("holding_{}", point_index),
                        format!("Holding Register {}", point_index),
                        ModbusRegisterType::HoldingRegister,
                        point_index,
                        DataType::UInt16,
                    ))
                    .with_point(PointProfile::new(
                        format!("input_{}", point_index),
                        format!("Input Register {}", point_index),
                        ModbusRegisterType::InputRegister,
                        point_index,
                        DataType::UInt16,
                    ))
                    .with_point(PointProfile::new(
                        format!("coil_{}", point_index),
                        format!("Coil {}", point_index),
                        ModbusRegisterType::Coil,
                        point_index,
                        DataType::Bool,
                    ))
                    .with_point(PointProfile::new(
                        format!("discrete_{}", point_index),
                        format!("Discrete Input {}", point_index),
                        ModbusRegisterType::DiscreteInput,
                        point_index,
                        DataType::Bool,
                    ));
            }

            profile.units.push(unit);
        }

        profile
    }
}

fn default_true() -> bool {
    true
}

#[allow(dead_code)]
fn _ensure_profile_result_type(_: ModbusResult<SimulatorProfile>) {}
