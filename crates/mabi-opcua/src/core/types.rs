use bytes::{Bytes, BytesMut};

use crate::codec::decoder::BinaryDecodable;
use crate::codec::encoder::BinaryEncodable;
use crate::core::registry::ServiceContext;
use crate::types::NodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ServiceId {
    GetEndpoints,
    CreateSession,
    ActivateSession,
    CloseSession,
    Read,
    Write,
    Browse,
    BrowseNext,
    CreateSubscription,
    DeleteSubscriptions,
    Publish,
    CreateMonitoredItems,
    DeleteMonitoredItems,
    ModifyMonitoredItems,
    RegisterNodes,
    UnregisterNodes,
    TranslateBrowsePaths,
    TransferSubscriptions,
    HistoryRead,
    Call,
}

impl ServiceId {
    pub(crate) const ALL: [ServiceId; 20] = [
        ServiceId::GetEndpoints,
        ServiceId::CreateSession,
        ServiceId::ActivateSession,
        ServiceId::CloseSession,
        ServiceId::Read,
        ServiceId::Write,
        ServiceId::Browse,
        ServiceId::BrowseNext,
        ServiceId::CreateSubscription,
        ServiceId::DeleteSubscriptions,
        ServiceId::Publish,
        ServiceId::CreateMonitoredItems,
        ServiceId::DeleteMonitoredItems,
        ServiceId::ModifyMonitoredItems,
        ServiceId::RegisterNodes,
        ServiceId::UnregisterNodes,
        ServiceId::TranslateBrowsePaths,
        ServiceId::TransferSubscriptions,
        ServiceId::HistoryRead,
        ServiceId::Call,
    ];

    pub(crate) fn request_type_id(self) -> NodeId {
        NodeId::numeric(
            0,
            match self {
                ServiceId::GetEndpoints => 428,
                ServiceId::CreateSession => 461,
                ServiceId::ActivateSession => 467,
                ServiceId::CloseSession => 473,
                ServiceId::Read => 631,
                ServiceId::Write => 673,
                ServiceId::Browse => 527,
                ServiceId::BrowseNext => 533,
                ServiceId::CreateSubscription => 787,
                ServiceId::DeleteSubscriptions => 847,
                ServiceId::Publish => 826,
                ServiceId::CreateMonitoredItems => 751,
                ServiceId::DeleteMonitoredItems => 781,
                ServiceId::ModifyMonitoredItems => 763,
                ServiceId::RegisterNodes => 560,
                ServiceId::UnregisterNodes => 566,
                ServiceId::TranslateBrowsePaths => 554,
                ServiceId::TransferSubscriptions => 839,
                ServiceId::HistoryRead => 662,
                ServiceId::Call => 712,
            },
        )
    }

    pub(crate) fn response_type_id(self) -> NodeId {
        NodeId::numeric(
            0,
            match self {
                ServiceId::GetEndpoints => 431,
                ServiceId::CreateSession => 464,
                ServiceId::ActivateSession => 470,
                ServiceId::CloseSession => 476,
                ServiceId::Read => 634,
                ServiceId::Write => 676,
                ServiceId::Browse => 530,
                ServiceId::BrowseNext => 536,
                ServiceId::CreateSubscription => 790,
                ServiceId::DeleteSubscriptions => 850,
                ServiceId::Publish => 829,
                ServiceId::CreateMonitoredItems => 754,
                ServiceId::DeleteMonitoredItems => 784,
                ServiceId::ModifyMonitoredItems => 766,
                ServiceId::RegisterNodes => 563,
                ServiceId::UnregisterNodes => 569,
                ServiceId::TranslateBrowsePaths => 557,
                ServiceId::TransferSubscriptions => 842,
                ServiceId::HistoryRead => 665,
                ServiceId::Call => 715,
            },
        )
    }

    pub(crate) fn from_request_type_id(type_id: &NodeId) -> Option<Self> {
        let numeric = type_id.as_numeric()?;
        Some(match numeric {
            428 => ServiceId::GetEndpoints,
            461 => ServiceId::CreateSession,
            467 => ServiceId::ActivateSession,
            473 => ServiceId::CloseSession,
            631 => ServiceId::Read,
            673 => ServiceId::Write,
            527 => ServiceId::Browse,
            533 => ServiceId::BrowseNext,
            787 => ServiceId::CreateSubscription,
            847 => ServiceId::DeleteSubscriptions,
            826 => ServiceId::Publish,
            751 => ServiceId::CreateMonitoredItems,
            781 => ServiceId::DeleteMonitoredItems,
            763 => ServiceId::ModifyMonitoredItems,
            560 => ServiceId::RegisterNodes,
            566 => ServiceId::UnregisterNodes,
            554 => ServiceId::TranslateBrowsePaths,
            839 => ServiceId::TransferSubscriptions,
            662 => ServiceId::HistoryRead,
            712 => ServiceId::Call,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) enum TypedRequest {
    GetEndpoints(Vec<u8>),
    CreateSession(Vec<u8>),
    ActivateSession(Vec<u8>),
    CloseSession(Vec<u8>),
    Read(Vec<u8>),
    Write(Vec<u8>),
    Browse(Vec<u8>),
    BrowseNext(Vec<u8>),
    CreateSubscription(Vec<u8>),
    DeleteSubscriptions(Vec<u8>),
    Publish(Vec<u8>),
    CreateMonitoredItems(Vec<u8>),
    DeleteMonitoredItems(Vec<u8>),
    ModifyMonitoredItems(Vec<u8>),
    RegisterNodes(Vec<u8>),
    UnregisterNodes(Vec<u8>),
    TranslateBrowsePaths(Vec<u8>),
    TransferSubscriptions(Vec<u8>),
    HistoryRead(Vec<u8>),
    Call(Vec<u8>),
}

impl TypedRequest {
    pub(crate) fn service_id(&self) -> ServiceId {
        match self {
            TypedRequest::GetEndpoints(_) => ServiceId::GetEndpoints,
            TypedRequest::CreateSession(_) => ServiceId::CreateSession,
            TypedRequest::ActivateSession(_) => ServiceId::ActivateSession,
            TypedRequest::CloseSession(_) => ServiceId::CloseSession,
            TypedRequest::Read(_) => ServiceId::Read,
            TypedRequest::Write(_) => ServiceId::Write,
            TypedRequest::Browse(_) => ServiceId::Browse,
            TypedRequest::BrowseNext(_) => ServiceId::BrowseNext,
            TypedRequest::CreateSubscription(_) => ServiceId::CreateSubscription,
            TypedRequest::DeleteSubscriptions(_) => ServiceId::DeleteSubscriptions,
            TypedRequest::Publish(_) => ServiceId::Publish,
            TypedRequest::CreateMonitoredItems(_) => ServiceId::CreateMonitoredItems,
            TypedRequest::DeleteMonitoredItems(_) => ServiceId::DeleteMonitoredItems,
            TypedRequest::ModifyMonitoredItems(_) => ServiceId::ModifyMonitoredItems,
            TypedRequest::RegisterNodes(_) => ServiceId::RegisterNodes,
            TypedRequest::UnregisterNodes(_) => ServiceId::UnregisterNodes,
            TypedRequest::TranslateBrowsePaths(_) => ServiceId::TranslateBrowsePaths,
            TypedRequest::TransferSubscriptions(_) => ServiceId::TransferSubscriptions,
            TypedRequest::HistoryRead(_) => ServiceId::HistoryRead,
            TypedRequest::Call(_) => ServiceId::Call,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum TypedResponse {
    GetEndpoints(Vec<u8>),
    CreateSession(Vec<u8>),
    ActivateSession(Vec<u8>),
    CloseSession(Vec<u8>),
    Read(Vec<u8>),
    Write(Vec<u8>),
    Browse(Vec<u8>),
    BrowseNext(Vec<u8>),
    CreateSubscription(Vec<u8>),
    DeleteSubscriptions(Vec<u8>),
    Publish(Vec<u8>),
    CreateMonitoredItems(Vec<u8>),
    DeleteMonitoredItems(Vec<u8>),
    ModifyMonitoredItems(Vec<u8>),
    RegisterNodes(Vec<u8>),
    UnregisterNodes(Vec<u8>),
    TranslateBrowsePaths(Vec<u8>),
    TransferSubscriptions(Vec<u8>),
    HistoryRead(Vec<u8>),
    Call(Vec<u8>),
}

impl TypedResponse {
    pub(crate) fn service_id(&self) -> ServiceId {
        match self {
            TypedResponse::GetEndpoints(_) => ServiceId::GetEndpoints,
            TypedResponse::CreateSession(_) => ServiceId::CreateSession,
            TypedResponse::ActivateSession(_) => ServiceId::ActivateSession,
            TypedResponse::CloseSession(_) => ServiceId::CloseSession,
            TypedResponse::Read(_) => ServiceId::Read,
            TypedResponse::Write(_) => ServiceId::Write,
            TypedResponse::Browse(_) => ServiceId::Browse,
            TypedResponse::BrowseNext(_) => ServiceId::BrowseNext,
            TypedResponse::CreateSubscription(_) => ServiceId::CreateSubscription,
            TypedResponse::DeleteSubscriptions(_) => ServiceId::DeleteSubscriptions,
            TypedResponse::Publish(_) => ServiceId::Publish,
            TypedResponse::CreateMonitoredItems(_) => ServiceId::CreateMonitoredItems,
            TypedResponse::DeleteMonitoredItems(_) => ServiceId::DeleteMonitoredItems,
            TypedResponse::ModifyMonitoredItems(_) => ServiceId::ModifyMonitoredItems,
            TypedResponse::RegisterNodes(_) => ServiceId::RegisterNodes,
            TypedResponse::UnregisterNodes(_) => ServiceId::UnregisterNodes,
            TypedResponse::TranslateBrowsePaths(_) => ServiceId::TranslateBrowsePaths,
            TypedResponse::TransferSubscriptions(_) => ServiceId::TransferSubscriptions,
            TypedResponse::HistoryRead(_) => ServiceId::HistoryRead,
            TypedResponse::Call(_) => ServiceId::Call,
        }
    }

    pub(crate) fn body(&self) -> &[u8] {
        match self {
            TypedResponse::GetEndpoints(body)
            | TypedResponse::CreateSession(body)
            | TypedResponse::ActivateSession(body)
            | TypedResponse::CloseSession(body)
            | TypedResponse::Read(body)
            | TypedResponse::Write(body)
            | TypedResponse::Browse(body)
            | TypedResponse::BrowseNext(body)
            | TypedResponse::CreateSubscription(body)
            | TypedResponse::DeleteSubscriptions(body)
            | TypedResponse::Publish(body)
            | TypedResponse::CreateMonitoredItems(body)
            | TypedResponse::DeleteMonitoredItems(body)
            | TypedResponse::ModifyMonitoredItems(body)
            | TypedResponse::RegisterNodes(body)
            | TypedResponse::UnregisterNodes(body)
            | TypedResponse::TranslateBrowsePaths(body)
            | TypedResponse::TransferSubscriptions(body)
            | TypedResponse::HistoryRead(body)
            | TypedResponse::Call(body) => body.as_slice(),
        }
    }
}

pub(crate) type DispatchContext = ServiceContext;

pub(crate) fn decode_request_type_id(payload: &[u8]) -> crate::error::OpcUaResult<NodeId> {
    let mut buf = Bytes::copy_from_slice(payload);
    NodeId::decode(&mut buf)
}

pub(crate) fn encode_response_payload(
    response: &TypedResponse,
) -> crate::error::OpcUaResult<Vec<u8>> {
    let mut out = BytesMut::new();
    response.service_id().response_type_id().encode(&mut out)?;
    out.extend_from_slice(response.body());
    Ok(out.to_vec())
}
