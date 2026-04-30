use std::sync::Arc;

use mabi_knx::{DptId, GroupAddress, GroupObjectTable, KnxResult};

pub const SWITCH: &str = "1/0/1";
pub const SCALING: &str = "1/0/2";
pub const TEMPERATURE: &str = "1/0/3";
pub const COUNTER: &str = "1/0/4";
pub const SIGNED_COUNTER: &str = "1/0/5";
pub const FLOAT: &str = "1/0/6";
pub const TEXT: &str = "1/0/7";
pub const HVAC: &str = "1/0/8";
pub const RGB: &str = "1/0/9";

pub fn standard_group_table() -> KnxResult<Arc<GroupObjectTable>> {
    let table = GroupObjectTable::new();
    table.create(SWITCH.parse::<GroupAddress>()?, "Switch", &DptId::new(1, 1))?;
    table.create(
        SCALING.parse::<GroupAddress>()?,
        "Scaling",
        &DptId::new(5, 1),
    )?;
    table.create(
        TEMPERATURE.parse::<GroupAddress>()?,
        "Temperature",
        &DptId::new(9, 1),
    )?;
    table.create(
        COUNTER.parse::<GroupAddress>()?,
        "Counter",
        &DptId::new(12, 1),
    )?;
    table.create(
        SIGNED_COUNTER.parse::<GroupAddress>()?,
        "SignedCounter",
        &DptId::new(13, 1),
    )?;
    table.create(FLOAT.parse::<GroupAddress>()?, "Float", &DptId::new(14, 56))?;
    table.create(TEXT.parse::<GroupAddress>()?, "Text", &DptId::new(16, 1))?;
    table.create(
        HVAC.parse::<GroupAddress>()?,
        "HvacMode",
        &DptId::new(20, 102),
    )?;
    table.create(RGB.parse::<GroupAddress>()?, "Rgb", &DptId::new(232, 600))?;
    Ok(Arc::new(table))
}
