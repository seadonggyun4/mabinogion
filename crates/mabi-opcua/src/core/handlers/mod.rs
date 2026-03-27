pub(crate) mod registry {
    pub(crate) use crate::core::registry::{
        ServiceContext, ServiceHandler, ServiceRegistry, ServiceResponse,
    };
}

#[allow(dead_code)]
#[path = "../../service/attribute.rs"]
pub(crate) mod attribute;
#[allow(dead_code)]
#[path = "../../service/browse.rs"]
pub(crate) mod browse;
#[allow(dead_code)]
#[path = "../../service/discovery.rs"]
pub(crate) mod discovery;
#[allow(dead_code)]
#[path = "../../service/history.rs"]
pub(crate) mod history;
#[allow(dead_code)]
#[path = "../../service/method_call.rs"]
pub(crate) mod method_call;
#[allow(dead_code)]
#[path = "../../service/monitored_item.rs"]
pub(crate) mod monitored_item;
#[allow(dead_code)]
#[path = "../../service/register_nodes.rs"]
pub(crate) mod register_nodes;
#[allow(dead_code)]
#[path = "../../service/session.rs"]
pub(crate) mod session;
#[allow(dead_code)]
#[path = "../../service/subscription.rs"]
pub(crate) mod subscription;
#[allow(dead_code)]
#[path = "../../service/transfer_subscription.rs"]
pub(crate) mod transfer_subscription;
#[allow(dead_code)]
#[path = "../../service/translate_browse_paths.rs"]
pub(crate) mod translate_browse_paths;
