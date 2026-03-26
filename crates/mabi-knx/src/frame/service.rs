//! KNXnet/IP service types.

use crate::error::KnxError;

/// KNXnet/IP service type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum ServiceType {
    // ========================================================================
    // Core Services (0x02xx)
    // ========================================================================
    /// Search Request.
    SearchRequest = 0x0201,
    /// Search Response.
    SearchResponse = 0x0202,
    /// Description Request.
    DescriptionRequest = 0x0203,
    /// Description Response.
    DescriptionResponse = 0x0204,
    /// Connect Request.
    ConnectRequest = 0x0205,
    /// Connect Response.
    ConnectResponse = 0x0206,
    /// Connection State Request.
    ConnectionStateRequest = 0x0207,
    /// Connection State Response.
    ConnectionStateResponse = 0x0208,
    /// Disconnect Request.
    DisconnectRequest = 0x0209,
    /// Disconnect Response.
    DisconnectResponse = 0x020A,
    /// Search Request Extended.
    SearchRequestExtended = 0x020B,
    /// Search Response Extended.
    SearchResponseExtended = 0x020C,

    // ========================================================================
    // Device Management (0x03xx)
    // ========================================================================
    /// Device Configuration Request.
    DeviceConfigurationRequest = 0x0310,
    /// Device Configuration ACK.
    DeviceConfigurationAck = 0x0311,

    // ========================================================================
    // Tunnelling (0x04xx)
    // ========================================================================
    /// Tunnelling Request.
    TunnellingRequest = 0x0420,
    /// Tunnelling ACK.
    TunnellingAck = 0x0421,
    /// Tunnelling Feature Get.
    TunnellingFeatureGet = 0x0422,
    /// Tunnelling Feature Response.
    TunnellingFeatureResponse = 0x0423,
    /// Tunnelling Feature Set.
    TunnellingFeatureSet = 0x0424,
    /// Tunnelling Feature Info.
    TunnellingFeatureInfo = 0x0425,

    // ========================================================================
    // Routing (0x05xx)
    // ========================================================================
    /// Routing Indication.
    RoutingIndication = 0x0530,
    /// Routing Lost Message.
    RoutingLostMessage = 0x0531,
    /// Routing Busy.
    RoutingBusy = 0x0532,
    /// Routing System Broadcast.
    RoutingSystemBroadcast = 0x0533,

    // ========================================================================
    // Remote Logging (0x06xx)
    // ========================================================================
    /// Remote Diagnostic Request.
    RemoteDiagnosticRequest = 0x0740,
    /// Remote Diagnostic Response.
    RemoteDiagnosticResponse = 0x0741,
    /// Remote Basic Configuration Request.
    RemoteBasicConfigRequest = 0x0742,
    /// Remote Reset Request.
    RemoteResetRequest = 0x0743,

    // ========================================================================
    // Object Server (0x08xx)
    // ========================================================================
    /// Object Server Request.
    ObjectServerRequest = 0x0800,
}

impl ServiceType {
    /// Get the service family.
    pub fn family(&self) -> ServiceFamily {
        match (*self as u16) >> 8 {
            0x02 => ServiceFamily::Core,
            0x03 => ServiceFamily::DeviceManagement,
            0x04 => ServiceFamily::Tunnelling,
            0x05 => ServiceFamily::Routing,
            0x06 => ServiceFamily::RemoteLogging,
            0x07 => ServiceFamily::RemoteConfiguration,
            0x08 => ServiceFamily::ObjectServer,
            _ => ServiceFamily::Unknown,
        }
    }

    /// Check if this is a request.
    pub fn is_request(&self) -> bool {
        matches!(
            self,
            Self::SearchRequest
                | Self::DescriptionRequest
                | Self::ConnectRequest
                | Self::ConnectionStateRequest
                | Self::DisconnectRequest
                | Self::DeviceConfigurationRequest
                | Self::TunnellingRequest
                | Self::TunnellingFeatureGet
                | Self::TunnellingFeatureSet
                | Self::RemoteDiagnosticRequest
                | Self::RemoteBasicConfigRequest
                | Self::RemoteResetRequest
                | Self::ObjectServerRequest
        )
    }

    /// Check if this is a response.
    pub fn is_response(&self) -> bool {
        matches!(
            self,
            Self::SearchResponse
                | Self::DescriptionResponse
                | Self::ConnectResponse
                | Self::ConnectionStateResponse
                | Self::DisconnectResponse
                | Self::TunnellingFeatureResponse
                | Self::RemoteDiagnosticResponse
        )
    }

    /// Check if this is an ACK.
    pub fn is_ack(&self) -> bool {
        matches!(self, Self::DeviceConfigurationAck | Self::TunnellingAck)
    }

    /// Check if this is a tunnelling service.
    pub fn is_tunnelling(&self) -> bool {
        matches!(self.family(), ServiceFamily::Tunnelling)
    }

    /// Check if this is a routing service.
    pub fn is_routing(&self) -> bool {
        matches!(self.family(), ServiceFamily::Routing)
    }

    /// Check if this is a core service.
    pub fn is_core(&self) -> bool {
        matches!(self.family(), ServiceFamily::Core)
    }

    /// Get the expected response type for this request.
    pub fn expected_response(&self) -> Option<Self> {
        match self {
            Self::SearchRequest => Some(Self::SearchResponse),
            Self::DescriptionRequest => Some(Self::DescriptionResponse),
            Self::ConnectRequest => Some(Self::ConnectResponse),
            Self::ConnectionStateRequest => Some(Self::ConnectionStateResponse),
            Self::DisconnectRequest => Some(Self::DisconnectResponse),
            Self::TunnellingRequest => Some(Self::TunnellingAck),
            Self::DeviceConfigurationRequest => Some(Self::DeviceConfigurationAck),
            Self::TunnellingFeatureGet => Some(Self::TunnellingFeatureResponse),
            _ => None,
        }
    }
}

impl TryFrom<u16> for ServiceType {
    type Error = KnxError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            // Core
            0x0201 => Ok(Self::SearchRequest),
            0x0202 => Ok(Self::SearchResponse),
            0x0203 => Ok(Self::DescriptionRequest),
            0x0204 => Ok(Self::DescriptionResponse),
            0x0205 => Ok(Self::ConnectRequest),
            0x0206 => Ok(Self::ConnectResponse),
            0x0207 => Ok(Self::ConnectionStateRequest),
            0x0208 => Ok(Self::ConnectionStateResponse),
            0x0209 => Ok(Self::DisconnectRequest),
            0x020A => Ok(Self::DisconnectResponse),
            0x020B => Ok(Self::SearchRequestExtended),
            0x020C => Ok(Self::SearchResponseExtended),
            // Device Management
            0x0310 => Ok(Self::DeviceConfigurationRequest),
            0x0311 => Ok(Self::DeviceConfigurationAck),
            // Tunnelling
            0x0420 => Ok(Self::TunnellingRequest),
            0x0421 => Ok(Self::TunnellingAck),
            0x0422 => Ok(Self::TunnellingFeatureGet),
            0x0423 => Ok(Self::TunnellingFeatureResponse),
            0x0424 => Ok(Self::TunnellingFeatureSet),
            0x0425 => Ok(Self::TunnellingFeatureInfo),
            // Routing
            0x0530 => Ok(Self::RoutingIndication),
            0x0531 => Ok(Self::RoutingLostMessage),
            0x0532 => Ok(Self::RoutingBusy),
            0x0533 => Ok(Self::RoutingSystemBroadcast),
            // Remote
            0x0740 => Ok(Self::RemoteDiagnosticRequest),
            0x0741 => Ok(Self::RemoteDiagnosticResponse),
            0x0742 => Ok(Self::RemoteBasicConfigRequest),
            0x0743 => Ok(Self::RemoteResetRequest),
            // Object Server
            0x0800 => Ok(Self::ObjectServerRequest),
            _ => Err(KnxError::UnknownServiceType(value)),
        }
    }
}

impl From<ServiceType> for u16 {
    fn from(st: ServiceType) -> Self {
        st as u16
    }
}

/// Service family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceFamily {
    Core,
    DeviceManagement,
    Tunnelling,
    Routing,
    RemoteLogging,
    RemoteConfiguration,
    ObjectServer,
    Unknown,
}

impl ServiceFamily {
    /// Get the family ID.
    pub fn id(&self) -> u8 {
        match self {
            Self::Core => 0x02,
            Self::DeviceManagement => 0x03,
            Self::Tunnelling => 0x04,
            Self::Routing => 0x05,
            Self::RemoteLogging => 0x06,
            Self::RemoteConfiguration => 0x07,
            Self::ObjectServer => 0x08,
            Self::Unknown => 0x00,
        }
    }

    /// Get from family ID.
    pub fn from_id(id: u8) -> Self {
        match id {
            0x02 => Self::Core,
            0x03 => Self::DeviceManagement,
            0x04 => Self::Tunnelling,
            0x05 => Self::Routing,
            0x06 => Self::RemoteLogging,
            0x07 => Self::RemoteConfiguration,
            0x08 => Self::ObjectServer,
            _ => Self::Unknown,
        }
    }

    /// Get the name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Core => "KNXnet/IP Core",
            Self::DeviceManagement => "KNXnet/IP Device Management",
            Self::Tunnelling => "KNXnet/IP Tunnelling",
            Self::Routing => "KNXnet/IP Routing",
            Self::RemoteLogging => "KNXnet/IP Remote Logging",
            Self::RemoteConfiguration => "KNXnet/IP Remote Configuration",
            Self::ObjectServer => "KNXnet/IP Object Server",
            Self::Unknown => "Unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_type_conversion() {
        assert_eq!(
            ServiceType::try_from(0x0201).unwrap(),
            ServiceType::SearchRequest
        );
        assert_eq!(u16::from(ServiceType::SearchRequest), 0x0201);
    }

    #[test]
    fn test_service_type_family() {
        assert_eq!(ServiceType::SearchRequest.family(), ServiceFamily::Core);
        assert_eq!(
            ServiceType::TunnellingRequest.family(),
            ServiceFamily::Tunnelling
        );
        assert_eq!(
            ServiceType::RoutingIndication.family(),
            ServiceFamily::Routing
        );
    }

    #[test]
    fn test_service_type_is_request() {
        assert!(ServiceType::SearchRequest.is_request());
        assert!(ServiceType::ConnectRequest.is_request());
        assert!(!ServiceType::SearchResponse.is_request());
    }

    #[test]
    fn test_expected_response() {
        assert_eq!(
            ServiceType::ConnectRequest.expected_response(),
            Some(ServiceType::ConnectResponse)
        );
        assert_eq!(
            ServiceType::TunnellingRequest.expected_response(),
            Some(ServiceType::TunnellingAck)
        );
    }

    #[test]
    fn test_unknown_service_type() {
        assert!(ServiceType::try_from(0xFFFF).is_err());
    }
}
