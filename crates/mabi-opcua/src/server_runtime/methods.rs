use std::sync::Arc;

use crate::nodes::classes::{MethodNode, VariableNode};
use crate::nodes::reference::Reference;
use crate::nodes::AddressSpace;
use crate::sdk::methods::MethodRegistry;
use crate::types::{NodeId, StatusCode, Variant};

use super::MethodRegistryPreset;

pub(crate) fn build_method_registry(
    preset: MethodRegistryPreset,
    address_space: &Arc<AddressSpace>,
) -> Arc<MethodRegistry> {
    let registry = Arc::new(MethodRegistry::new());
    match preset {
        MethodRegistryPreset::Default => register_default_methods(address_space, &registry),
        MethodRegistryPreset::Empty => {}
    }
    registry
}

fn register_default_methods(
    address_space: &Arc<AddressSpace>,
    method_registry: &Arc<MethodRegistry>,
) {
    let method_node_id = NodeId::numeric(0, 62541);

    let method_node = MethodNode::new(method_node_id.clone(), "Multiply", "Multiply");
    address_space.insert_node(method_node);
    address_space.add_reference(Reference::has_component(
        NodeId::server(),
        method_node_id.clone(),
    ));

    let input_args_id = NodeId::numeric(0, 62542);
    let input_args_var = VariableNode::new(
        input_args_id.clone(),
        "InputArguments",
        "InputArguments",
        NodeId::numeric(0, 296),
        Variant::Null,
    );
    address_space.insert_node(input_args_var);
    address_space.add_reference(Reference::has_property(
        method_node_id.clone(),
        input_args_id,
    ));

    let output_args_id = NodeId::numeric(0, 62543);
    let output_args_var = VariableNode::new(
        output_args_id.clone(),
        "OutputArguments",
        "OutputArguments",
        NodeId::numeric(0, 296),
        Variant::Null,
    );
    address_space.insert_node(output_args_var);
    address_space.add_reference(Reference::has_property(
        method_node_id.clone(),
        output_args_id,
    ));

    method_registry.register(
        method_node_id,
        Arc::new(|args: &[Variant]| {
            if args.len() < 2 {
                return Err(StatusCode::BAD_INVALID_ARGUMENT);
            }
            let a = args[0].as_f64().ok_or(StatusCode::BAD_INVALID_ARGUMENT)?;
            let b = args[1].as_f64().ok_or(StatusCode::BAD_INVALID_ARGUMENT)?;
            Ok(vec![Variant::Double(a * b)])
        }),
    );
}
