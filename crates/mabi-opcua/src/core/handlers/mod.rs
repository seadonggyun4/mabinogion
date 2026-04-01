pub(crate) mod registry {
    pub(crate) use crate::core::registry::{
        ServiceContext, ServiceHandler, ServiceRegistry, ServiceResponse,
    };
}

pub(crate) mod attribute;
pub(crate) mod browse;
pub(crate) mod discovery;
pub(crate) mod history;
pub(crate) mod method_call;
pub(crate) mod monitored_item;
pub(crate) mod register_nodes;
pub(crate) mod session;
pub(crate) mod subscription;
pub(crate) mod transfer_subscription;
pub(crate) mod translate_browse_paths;
