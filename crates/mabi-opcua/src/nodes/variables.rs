//! Variable node factories and helpers.
//!
//! This module provides factory methods for creating commonly used
//! variable types with proper configuration.

use crate::types::{NodeId, AccessLevel, Variant, variant::DataTypeId};
use super::base::{QualifiedName, LocalizedText};
use super::classes::VariableNode;

/// Factory for creating variable nodes.
///
/// Provides convenient methods for creating variables of different
/// data types with common configurations.
///
/// # Examples
///
/// ```ignore
/// // Create a temperature variable
/// let temp = VariableFactory::double(
///     NodeId::numeric(2, 1001),
///     "Temperature",
///     25.5,
/// );
///
/// // Create a boolean status variable
/// let status = VariableFactory::boolean(
///     NodeId::numeric(2, 1002),
///     "IsRunning",
///     true,
/// );
/// ```
pub struct VariableFactory;

impl VariableFactory {
    /// Create a Boolean variable.
    pub fn boolean(
        node_id: NodeId,
        name: impl Into<String>,
        value: bool,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::Boolean,
            Variant::boolean(value),
        )
    }

    /// Create a writable Boolean variable.
    pub fn boolean_writable(
        node_id: NodeId,
        name: impl Into<String>,
        value: bool,
    ) -> VariableNode {
        Self::boolean(node_id, name, value).writable()
    }

    /// Create an SByte variable.
    pub fn sbyte(
        node_id: NodeId,
        name: impl Into<String>,
        value: i8,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::SByte,
            Variant::sbyte(value),
        )
    }

    /// Create a Byte variable.
    pub fn byte(
        node_id: NodeId,
        name: impl Into<String>,
        value: u8,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::Byte,
            Variant::byte(value),
        )
    }

    /// Create an Int16 variable.
    pub fn int16(
        node_id: NodeId,
        name: impl Into<String>,
        value: i16,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::Int16,
            Variant::int16(value),
        )
    }

    /// Create a UInt16 variable.
    pub fn uint16(
        node_id: NodeId,
        name: impl Into<String>,
        value: u16,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::UInt16,
            Variant::uint16(value),
        )
    }

    /// Create an Int32 variable.
    pub fn int32(
        node_id: NodeId,
        name: impl Into<String>,
        value: i32,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::Int32,
            Variant::int32(value),
        )
    }

    /// Create a writable Int32 variable.
    pub fn int32_writable(
        node_id: NodeId,
        name: impl Into<String>,
        value: i32,
    ) -> VariableNode {
        Self::int32(node_id, name, value).writable()
    }

    /// Create a UInt32 variable.
    pub fn uint32(
        node_id: NodeId,
        name: impl Into<String>,
        value: u32,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::UInt32,
            Variant::uint32(value),
        )
    }

    /// Create an Int64 variable.
    pub fn int64(
        node_id: NodeId,
        name: impl Into<String>,
        value: i64,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::Int64,
            Variant::int64(value),
        )
    }

    /// Create a UInt64 variable.
    pub fn uint64(
        node_id: NodeId,
        name: impl Into<String>,
        value: u64,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::UInt64,
            Variant::uint64(value),
        )
    }

    /// Create a Float variable.
    pub fn float(
        node_id: NodeId,
        name: impl Into<String>,
        value: f32,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::Float,
            Variant::float(value),
        )
    }

    /// Create a writable Float variable.
    pub fn float_writable(
        node_id: NodeId,
        name: impl Into<String>,
        value: f32,
    ) -> VariableNode {
        Self::float(node_id, name, value).writable()
    }

    /// Create a Double variable.
    pub fn double(
        node_id: NodeId,
        name: impl Into<String>,
        value: f64,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::Double,
            Variant::double(value),
        )
    }

    /// Create a writable Double variable.
    pub fn double_writable(
        node_id: NodeId,
        name: impl Into<String>,
        value: f64,
    ) -> VariableNode {
        Self::double(node_id, name, value).writable()
    }

    /// Create a String variable.
    pub fn string(
        node_id: NodeId,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::String,
            Variant::string(value),
        )
    }

    /// Create a writable String variable.
    pub fn string_writable(
        node_id: NodeId,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> VariableNode {
        Self::string(node_id, name, value).writable()
    }

    /// Create a DateTime variable with current time.
    pub fn datetime_now(
        node_id: NodeId,
        name: impl Into<String>,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::DateTime,
            Variant::datetime(chrono::Utc::now()),
        )
    }

    /// Create an array of Double values.
    pub fn double_array(
        node_id: NodeId,
        name: impl Into<String>,
        values: Vec<f64>,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::Double,
            Variant::double_array(values),
        )
        .with_array(1, None)
    }

    /// Create an array of Int32 values.
    pub fn int32_array(
        node_id: NodeId,
        name: impl Into<String>,
        values: Vec<i32>,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::Int32,
            Variant::int32_array(values),
        )
        .with_array(1, None)
    }

    /// Create an array of String values.
    pub fn string_array(
        node_id: NodeId,
        name: impl Into<String>,
        values: Vec<String>,
    ) -> VariableNode {
        let name = name.into();
        VariableNode::with_type_id(
            node_id.clone(),
            QualifiedName::new(node_id.namespace(), &name),
            LocalizedText::invariant(&name),
            DataTypeId::String,
            Variant::string_array(values),
        )
        .with_array(1, None)
    }
}

/// Analog variable with engineering units and range.
///
/// Represents a physical measurement with unit information.
#[derive(Debug, Clone)]
pub struct AnalogVariable {
    /// The underlying variable node.
    pub variable: VariableNode,
    /// Engineering unit display name.
    pub unit_name: String,
    /// Engineering unit description.
    pub unit_description: String,
    /// Low range value.
    pub range_low: f64,
    /// High range value.
    pub range_high: f64,
}

impl AnalogVariable {
    /// Create a new analog variable.
    pub fn new(
        node_id: NodeId,
        name: impl Into<String>,
        value: f64,
    ) -> Self {
        let name = name.into();
        Self {
            variable: VariableFactory::double(node_id, &name, value),
            unit_name: String::new(),
            unit_description: String::new(),
            range_low: 0.0,
            range_high: 100.0,
        }
    }

    /// Set the engineering unit.
    pub fn with_unit(mut self, name: impl Into<String>, description: impl Into<String>) -> Self {
        self.unit_name = name.into();
        self.unit_description = description.into();
        self
    }

    /// Set the engineering unit range.
    pub fn with_range(mut self, low: f64, high: f64) -> Self {
        self.range_low = low;
        self.range_high = high;
        self
    }

    /// Make writable.
    pub fn writable(mut self) -> Self {
        self.variable = self.variable.writable();
        self
    }

    /// Enable historizing.
    pub fn historizing(mut self) -> Self {
        self.variable = self.variable.with_historizing(true);
        self
    }

    /// Get the variable node.
    pub fn into_variable(self) -> VariableNode {
        self.variable
    }

    /// Common temperature variable (Celsius).
    pub fn temperature(node_id: NodeId, name: impl Into<String>, value: f64) -> Self {
        Self::new(node_id, name, value)
            .with_unit("°C", "Degrees Celsius")
            .with_range(-40.0, 100.0)
    }

    /// Common pressure variable (kPa).
    pub fn pressure(node_id: NodeId, name: impl Into<String>, value: f64) -> Self {
        Self::new(node_id, name, value)
            .with_unit("kPa", "Kilopascals")
            .with_range(0.0, 1000.0)
    }

    /// Common flow rate variable (L/min).
    pub fn flow_rate(node_id: NodeId, name: impl Into<String>, value: f64) -> Self {
        Self::new(node_id, name, value)
            .with_unit("L/min", "Liters per minute")
            .with_range(0.0, 1000.0)
    }

    /// Common percentage variable.
    pub fn percentage(node_id: NodeId, name: impl Into<String>, value: f64) -> Self {
        Self::new(node_id, name, value)
            .with_unit("%", "Percent")
            .with_range(0.0, 100.0)
    }

    /// Common power variable (kW).
    pub fn power(node_id: NodeId, name: impl Into<String>, value: f64) -> Self {
        Self::new(node_id, name, value)
            .with_unit("kW", "Kilowatts")
            .with_range(0.0, 10000.0)
    }

    /// Common voltage variable (V).
    pub fn voltage(node_id: NodeId, name: impl Into<String>, value: f64) -> Self {
        Self::new(node_id, name, value)
            .with_unit("V", "Volts")
            .with_range(0.0, 500.0)
    }

    /// Common current variable (A).
    pub fn current(node_id: NodeId, name: impl Into<String>, value: f64) -> Self {
        Self::new(node_id, name, value)
            .with_unit("A", "Amperes")
            .with_range(0.0, 100.0)
    }
}

/// Discrete (enumerated) variable.
///
/// Represents a variable with a fixed set of possible values.
#[derive(Debug, Clone)]
pub struct DiscreteVariable {
    /// The underlying variable node.
    pub variable: VariableNode,
    /// Enumeration values.
    pub enum_values: Vec<EnumValue>,
}

/// Enumeration value.
#[derive(Debug, Clone)]
pub struct EnumValue {
    /// Numeric value.
    pub value: i64,
    /// Display name.
    pub display_name: String,
    /// Description.
    pub description: String,
}

impl EnumValue {
    /// Create a new enum value.
    pub fn new(value: i64, display_name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            value,
            display_name: display_name.into(),
            description: description.into(),
        }
    }
}

impl DiscreteVariable {
    /// Create a new discrete variable.
    pub fn new(
        node_id: NodeId,
        name: impl Into<String>,
        value: i64,
        enum_values: Vec<EnumValue>,
    ) -> Self {
        let name = name.into();
        Self {
            variable: VariableFactory::int64(node_id, &name, value),
            enum_values,
        }
    }

    /// Make writable.
    pub fn writable(mut self) -> Self {
        self.variable = self.variable.writable();
        self
    }

    /// Get the variable node.
    pub fn into_variable(self) -> VariableNode {
        self.variable
    }

    /// Create a common on/off discrete variable.
    pub fn on_off(node_id: NodeId, name: impl Into<String>, is_on: bool) -> Self {
        Self::new(
            node_id,
            name,
            if is_on { 1 } else { 0 },
            vec![
                EnumValue::new(0, "Off", "Device is off"),
                EnumValue::new(1, "On", "Device is on"),
            ],
        )
    }

    /// Create a common running state variable.
    pub fn running_state(node_id: NodeId, name: impl Into<String>, state: i64) -> Self {
        Self::new(
            node_id,
            name,
            state,
            vec![
                EnumValue::new(0, "Stopped", "Device is stopped"),
                EnumValue::new(1, "Starting", "Device is starting"),
                EnumValue::new(2, "Running", "Device is running"),
                EnumValue::new(3, "Stopping", "Device is stopping"),
                EnumValue::new(4, "Fault", "Device has a fault"),
            ],
        )
    }

    /// Create a common alarm severity variable.
    pub fn alarm_severity(node_id: NodeId, name: impl Into<String>, severity: i64) -> Self {
        Self::new(
            node_id,
            name,
            severity,
            vec![
                EnumValue::new(0, "Normal", "No alarm"),
                EnumValue::new(1, "Low", "Low severity alarm"),
                EnumValue::new(2, "Medium", "Medium severity alarm"),
                EnumValue::new(3, "High", "High severity alarm"),
                EnumValue::new(4, "Critical", "Critical alarm"),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_factory_double() {
        let var = VariableFactory::double(
            NodeId::numeric(2, 1001),
            "Temperature",
            25.5,
        );

        assert_eq!(var.base.browse_name.name, "Temperature");
        assert!(!var.is_writable());
    }

    #[test]
    fn test_variable_factory_writable() {
        let var = VariableFactory::double_writable(
            NodeId::numeric(2, 1001),
            "Setpoint",
            22.0,
        );

        assert!(var.is_writable());
    }

    #[test]
    fn test_analog_variable() {
        let analog = AnalogVariable::temperature(
            NodeId::numeric(2, 1001),
            "RoomTemp",
            25.5,
        );

        assert_eq!(analog.unit_name, "°C");
        assert_eq!(analog.range_low, -40.0);
        assert_eq!(analog.range_high, 100.0);
    }

    #[test]
    fn test_discrete_variable() {
        let discrete = DiscreteVariable::on_off(
            NodeId::numeric(2, 1001),
            "Pump",
            true,
        );

        assert_eq!(discrete.enum_values.len(), 2);
    }

    #[test]
    fn test_array_variables() {
        let var = VariableFactory::double_array(
            NodeId::numeric(2, 1001),
            "Temperatures",
            vec![25.0, 26.0, 27.0],
        );

        assert_eq!(var.value_rank, 1);
    }
}
