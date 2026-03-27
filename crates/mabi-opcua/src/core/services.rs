use std::collections::BTreeSet;

use crate::core::handlers::{
    attribute, browse, discovery, history, method_call, monitored_item, register_nodes, session,
    subscription, transfer_subscription, translate_browse_paths,
};
use crate::core::registry::ServiceHandler;
use crate::core::status::ServiceError;
use crate::core::types::{DispatchContext, ServiceId, TypedRequest, TypedResponse};

#[derive(Debug, Clone, Default)]
pub(crate) struct BuiltinServiceSet {
    enabled: BTreeSet<ServiceId>,
}

impl BuiltinServiceSet {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn all() -> Self {
        Self {
            enabled: ServiceId::ALL.into_iter().collect(),
        }
    }

    pub(crate) fn enable(&mut self, service_id: ServiceId) {
        self.enabled.insert(service_id);
    }

    pub(crate) fn contains(&self, service_id: ServiceId) -> bool {
        self.enabled.contains(&service_id)
    }

    pub(crate) fn len(&self) -> usize {
        self.enabled.len()
    }

    pub(crate) async fn dispatch(
        &self,
        request: TypedRequest,
        context: &DispatchContext,
    ) -> Result<TypedResponse, ServiceError> {
        let service_id = request.service_id();
        if !self.contains(service_id) {
            return Err(ServiceError::UnsupportedService {
                type_id: service_id.request_type_id(),
            });
        }

        let body = match request {
            TypedRequest::GetEndpoints(body) => {
                discovery::GetEndpointsHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::CreateSession(body) => {
                session::CreateSessionHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::ActivateSession(body) => {
                session::ActivateSessionHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::CloseSession(body) => {
                session::CloseSessionHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::Read(body) => attribute::ReadHandler.handle(&body, context).await?.body,
            TypedRequest::Write(body) => attribute::WriteHandler.handle(&body, context).await?.body,
            TypedRequest::Browse(body) => browse::BrowseHandler.handle(&body, context).await?.body,
            TypedRequest::BrowseNext(body) => {
                browse::BrowseNextHandler.handle(&body, context).await?.body
            }
            TypedRequest::CreateSubscription(body) => {
                subscription::CreateSubscriptionHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::DeleteSubscriptions(body) => {
                subscription::DeleteSubscriptionsHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::Publish(body) => {
                subscription::PublishHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::CreateMonitoredItems(body) => {
                monitored_item::CreateMonitoredItemsHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::DeleteMonitoredItems(body) => {
                monitored_item::DeleteMonitoredItemsHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::ModifyMonitoredItems(body) => {
                monitored_item::ModifyMonitoredItemsHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::RegisterNodes(body) => {
                register_nodes::RegisterNodesHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::UnregisterNodes(body) => {
                register_nodes::UnregisterNodesHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::TranslateBrowsePaths(body) => {
                translate_browse_paths::TranslateBrowsePathsHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::TransferSubscriptions(body) => {
                transfer_subscription::TransferSubscriptionsHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::HistoryRead(body) => {
                history::HistoryReadHandler
                    .handle(&body, context)
                    .await?
                    .body
            }
            TypedRequest::Call(body) => method_call::CallHandler.handle(&body, context).await?.body,
        };

        Ok(match service_id {
            ServiceId::GetEndpoints => TypedResponse::GetEndpoints(body),
            ServiceId::CreateSession => TypedResponse::CreateSession(body),
            ServiceId::ActivateSession => TypedResponse::ActivateSession(body),
            ServiceId::CloseSession => TypedResponse::CloseSession(body),
            ServiceId::Read => TypedResponse::Read(body),
            ServiceId::Write => TypedResponse::Write(body),
            ServiceId::Browse => TypedResponse::Browse(body),
            ServiceId::BrowseNext => TypedResponse::BrowseNext(body),
            ServiceId::CreateSubscription => TypedResponse::CreateSubscription(body),
            ServiceId::DeleteSubscriptions => TypedResponse::DeleteSubscriptions(body),
            ServiceId::Publish => TypedResponse::Publish(body),
            ServiceId::CreateMonitoredItems => TypedResponse::CreateMonitoredItems(body),
            ServiceId::DeleteMonitoredItems => TypedResponse::DeleteMonitoredItems(body),
            ServiceId::ModifyMonitoredItems => TypedResponse::ModifyMonitoredItems(body),
            ServiceId::RegisterNodes => TypedResponse::RegisterNodes(body),
            ServiceId::UnregisterNodes => TypedResponse::UnregisterNodes(body),
            ServiceId::TranslateBrowsePaths => TypedResponse::TranslateBrowsePaths(body),
            ServiceId::TransferSubscriptions => TypedResponse::TransferSubscriptions(body),
            ServiceId::HistoryRead => TypedResponse::HistoryRead(body),
            ServiceId::Call => TypedResponse::Call(body),
        })
    }
}
