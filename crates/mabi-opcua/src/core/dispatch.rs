use crate::core::encoding::{decode_request_payload, encode_response};
use crate::core::services::BuiltinServiceSet;
use crate::core::status::ServiceError;
use crate::core::types::DispatchContext;

pub(crate) async fn dispatch_payload(
    payload: &[u8],
    context: &DispatchContext,
    services: &BuiltinServiceSet,
) -> Result<Vec<u8>, ServiceError> {
    let request = decode_request_payload(payload)?;
    let response = services.dispatch(request, context).await?;
    encode_response(&response)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bytes::{Bytes, BytesMut};

    use super::*;
    use crate::channel::secure_channel::SecureChannel;
    use crate::codec::data_value::ExtensionObject;
    use crate::codec::decoder::BinaryDecodable;
    use crate::codec::encoder::BinaryEncodable;
    use crate::config::OpcUaServerConfig;
    use crate::core::registry::ServiceContext;
    use crate::nodes::{AddressSpace, AddressSpaceConfig};
    use crate::sdk::history::{HistoryStore, HistoryStoreConfig};
    use crate::sdk::methods::MethodRegistry;
    use crate::sdk::session::{SessionManager, SessionManagerConfig};
    use crate::sdk::subscription::{SubscriptionManager, SubscriptionManagerConfig};
    use crate::security::{SecurityManager, SecurityManagerConfig};
    use crate::types::{NodeId, StatusCode};

    #[tokio::test]
    async fn built_in_dispatch_roundtrips_get_endpoints() {
        let context = test_context();
        let services = BuiltinServiceSet::all();
        let payload = encode_get_endpoints_payload(42);

        let response = dispatch_payload(&payload, &context, &services)
            .await
            .unwrap();

        let mut buf = Bytes::from(response);
        let type_id = NodeId::decode(&mut buf).unwrap();
        assert_eq!(type_id, NodeId::numeric(0, 431));

        let _timestamp = chrono::DateTime::<chrono::Utc>::decode(&mut buf).unwrap();
        let request_handle = u32::decode(&mut buf).unwrap();
        let status = StatusCode::decode(&mut buf).unwrap();

        assert_eq!(request_handle, 42);
        assert_eq!(status, StatusCode::GOOD);
    }

    fn encode_get_endpoints_payload(request_handle: u32) -> Vec<u8> {
        let mut out = BytesMut::new();
        NodeId::numeric(0, 428).encode(&mut out).unwrap();
        NodeId::numeric(0, 0).encode(&mut out).unwrap();
        chrono::Utc::now().encode(&mut out).unwrap();
        request_handle.encode(&mut out).unwrap();
        0u32.encode(&mut out).unwrap();
        "".to_string().encode(&mut out).unwrap();
        0u32.encode(&mut out).unwrap();
        ExtensionObject {
            type_id: NodeId::numeric(0, 0),
            body: None,
        }
        .encode(&mut out)
        .unwrap();
        "".to_string().encode(&mut out).unwrap();
        0i32.encode(&mut out).unwrap();
        0i32.encode(&mut out).unwrap();
        out.to_vec()
    }

    fn test_context() -> DispatchContext {
        ServiceContext {
            session_manager: Arc::new(SessionManager::with_config(SessionManagerConfig::default())),
            address_space: Arc::new(AddressSpace::new(AddressSpaceConfig::default())),
            subscription_manager: Arc::new(SubscriptionManager::with_config(
                SubscriptionManagerConfig::default(),
            )),
            history_store: Arc::new(HistoryStore::new(HistoryStoreConfig::default())),
            security_manager: Arc::new(SecurityManager::new(SecurityManagerConfig::default())),
            server_config: Arc::new(OpcUaServerConfig::default()),
            method_registry: Arc::new(MethodRegistry::new()),
            channel: Arc::new(SecureChannel::new_unsecured()),
            session_id: parking_lot::RwLock::new(None),
            auth_token: parking_lot::RwLock::new(None),
        }
    }
}
