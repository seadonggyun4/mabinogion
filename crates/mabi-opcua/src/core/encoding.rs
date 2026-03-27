use bytes::Bytes;

use crate::codec::decoder::BinaryDecodable;
use crate::core::status::ServiceError;
use crate::core::types::{
    decode_request_type_id, encode_response_payload, ServiceId, TypedRequest, TypedResponse,
};
use crate::types::NodeId;

pub(crate) fn peek_request_type_id(payload: &[u8]) -> crate::error::OpcUaResult<NodeId> {
    decode_request_type_id(payload)
}

pub(crate) fn decode_request_payload(payload: &[u8]) -> Result<TypedRequest, ServiceError> {
    let mut buf = Bytes::copy_from_slice(payload);
    let type_id = NodeId::decode(&mut buf)?;
    let service_id = ServiceId::from_request_type_id(&type_id).ok_or_else(|| {
        ServiceError::UnsupportedService {
            type_id: type_id.clone(),
        }
    })?;
    let body = buf.to_vec();

    Ok(match service_id {
        ServiceId::GetEndpoints => TypedRequest::GetEndpoints(body),
        ServiceId::CreateSession => TypedRequest::CreateSession(body),
        ServiceId::ActivateSession => TypedRequest::ActivateSession(body),
        ServiceId::CloseSession => TypedRequest::CloseSession(body),
        ServiceId::Read => TypedRequest::Read(body),
        ServiceId::Write => TypedRequest::Write(body),
        ServiceId::Browse => TypedRequest::Browse(body),
        ServiceId::BrowseNext => TypedRequest::BrowseNext(body),
        ServiceId::CreateSubscription => TypedRequest::CreateSubscription(body),
        ServiceId::DeleteSubscriptions => TypedRequest::DeleteSubscriptions(body),
        ServiceId::Publish => TypedRequest::Publish(body),
        ServiceId::CreateMonitoredItems => TypedRequest::CreateMonitoredItems(body),
        ServiceId::DeleteMonitoredItems => TypedRequest::DeleteMonitoredItems(body),
        ServiceId::ModifyMonitoredItems => TypedRequest::ModifyMonitoredItems(body),
        ServiceId::RegisterNodes => TypedRequest::RegisterNodes(body),
        ServiceId::UnregisterNodes => TypedRequest::UnregisterNodes(body),
        ServiceId::TranslateBrowsePaths => TypedRequest::TranslateBrowsePaths(body),
        ServiceId::TransferSubscriptions => TypedRequest::TransferSubscriptions(body),
        ServiceId::HistoryRead => TypedRequest::HistoryRead(body),
        ServiceId::Call => TypedRequest::Call(body),
    })
}

pub(crate) fn encode_response(response: &TypedResponse) -> Result<Vec<u8>, ServiceError> {
    encode_response_payload(response).map_err(ServiceError::from)
}
