//! Discovery services (Who-Is, I-Am).

use crate::apdu::encoding::{ApduDecoder, ApduEncoder};
use crate::apdu::types::UnconfirmedService;
use crate::object::property::SegmentationSupport;
use crate::object::types::{ObjectId, ObjectType};

use super::handler::{ServiceContext, ServiceResult, UnconfirmedServiceHandler};

/// Who-Is request.
#[derive(Debug, Clone)]
pub struct WhoIsRequest {
    /// Low range of device instance (inclusive).
    pub device_instance_low: Option<u32>,
    /// High range of device instance (inclusive).
    pub device_instance_high: Option<u32>,
}

impl WhoIsRequest {
    /// Create a request for all devices.
    pub fn all() -> Self {
        Self {
            device_instance_low: None,
            device_instance_high: None,
        }
    }

    /// Create a request for a specific range.
    pub fn range(low: u32, high: u32) -> Self {
        Self {
            device_instance_low: Some(low),
            device_instance_high: Some(high),
        }
    }

    /// Create a request for a specific device.
    pub fn specific(instance: u32) -> Self {
        Self::range(instance, instance)
    }

    /// Check if a device instance matches this request.
    pub fn matches(&self, device_instance: u32) -> bool {
        match (self.device_instance_low, self.device_instance_high) {
            (None, None) => true,
            (Some(low), None) => device_instance >= low,
            (None, Some(high)) => device_instance <= high,
            (Some(low), Some(high)) => device_instance >= low && device_instance <= high,
        }
    }

    /// Decode from APDU data.
    pub fn decode(data: &[u8]) -> Self {
        if data.is_empty() {
            return Self::all();
        }

        let mut decoder = ApduDecoder::new(data);

        let mut low = None;
        let mut high = None;

        // Try to decode context tag 0 (device instance low)
        if !decoder.is_empty() {
            if let Ok((tag, is_context, len)) = decoder.decode_tag_info() {
                if is_context && tag == 0 {
                    low = decoder.decode_unsigned(len).ok();
                }
            }
        }

        // Try to decode context tag 1 (device instance high)
        if !decoder.is_empty() {
            if let Ok((tag, is_context, len)) = decoder.decode_tag_info() {
                if is_context && tag == 1 {
                    high = decoder.decode_unsigned(len).ok();
                }
            }
        }

        Self {
            device_instance_low: low,
            device_instance_high: high,
        }
    }

    /// Encode to APDU data.
    pub fn encode(&self) -> Vec<u8> {
        let mut encoder = ApduEncoder::new();

        if let Some(low) = self.device_instance_low {
            encoder.encode_context_unsigned(0, low);
        }

        if let Some(high) = self.device_instance_high {
            encoder.encode_context_unsigned(1, high);
        }

        encoder.into_bytes()
    }
}

/// I-Am response.
#[derive(Debug, Clone)]
pub struct IAmResponse {
    /// Device object identifier.
    pub device_identifier: ObjectId,
    /// Maximum APDU length accepted.
    pub max_apdu_length_accepted: u16,
    /// Segmentation supported.
    pub segmentation_supported: SegmentationSupport,
    /// Vendor identifier.
    pub vendor_id: u16,
}

impl IAmResponse {
    /// Create a new I-Am response.
    pub fn new(
        device_instance: u32,
        max_apdu_length: u16,
        segmentation: SegmentationSupport,
        vendor_id: u16,
    ) -> Self {
        Self {
            device_identifier: ObjectId::new(ObjectType::Device, device_instance),
            max_apdu_length_accepted: max_apdu_length,
            segmentation_supported: segmentation,
            vendor_id,
        }
    }

    /// Encode to APDU data.
    pub fn encode(&self) -> Vec<u8> {
        let mut encoder = ApduEncoder::new();

        // I-Am Device Identifier
        encoder.encode_object_identifier(self.device_identifier);

        // Max APDU Length Accepted
        encoder.encode_unsigned(self.max_apdu_length_accepted as u32);

        // Segmentation Supported
        encoder.encode_enumerated(self.segmentation_supported as u32);

        // Vendor ID
        encoder.encode_unsigned(self.vendor_id as u32);

        encoder.into_bytes()
    }

    /// Decode from APDU data.
    pub fn decode(data: &[u8]) -> Option<Self> {
        let mut decoder = ApduDecoder::new(data);

        // Device Identifier
        let (tag, is_context, _len) = decoder.decode_tag_info().ok()?;
        if tag != 12 || is_context {
            return None;
        }
        let device_identifier = decoder.decode_object_identifier().ok()?;

        // Max APDU Length Accepted
        let (tag, is_context, len) = decoder.decode_tag_info().ok()?;
        if tag != 2 || is_context {
            return None;
        }
        let max_apdu_length_accepted = decoder.decode_unsigned(len).ok()? as u16;

        // Segmentation Supported
        let (tag, is_context, len) = decoder.decode_tag_info().ok()?;
        if tag != 9 || is_context {
            return None;
        }
        let seg_value = decoder.decode_unsigned(len).ok()?;
        let segmentation_supported = match seg_value {
            0 => SegmentationSupport::Both,
            1 => SegmentationSupport::Transmit,
            2 => SegmentationSupport::Receive,
            _ => SegmentationSupport::None,
        };

        // Vendor ID
        let (tag, is_context, len) = decoder.decode_tag_info().ok()?;
        if tag != 2 || is_context {
            return None;
        }
        let vendor_id = decoder.decode_unsigned(len).ok()? as u16;

        Some(Self {
            device_identifier,
            max_apdu_length_accepted,
            segmentation_supported,
            vendor_id,
        })
    }
}

/// Discovery service for handling Who-Is and I-Am.
pub struct DiscoveryService {
    /// Device instance number.
    pub device_instance: u32,
    /// Maximum APDU length.
    pub max_apdu_length: u16,
    /// Segmentation support.
    pub segmentation: SegmentationSupport,
    /// Vendor ID.
    pub vendor_id: u16,
}

impl DiscoveryService {
    /// Create a new discovery service.
    pub fn new(device_instance: u32, vendor_id: u16) -> Self {
        Self {
            device_instance,
            max_apdu_length: 1476,
            segmentation: SegmentationSupport::None,
            vendor_id,
        }
    }

    /// Handle a Who-Is request.
    pub fn handle_who_is(&self, request: &WhoIsRequest) -> Option<IAmResponse> {
        if request.matches(self.device_instance) {
            Some(IAmResponse::new(
                self.device_instance,
                self.max_apdu_length,
                self.segmentation,
                self.vendor_id,
            ))
        } else {
            None
        }
    }

    /// Generate an I-Am response.
    pub fn generate_i_am(&self) -> IAmResponse {
        IAmResponse::new(
            self.device_instance,
            self.max_apdu_length,
            self.segmentation,
            self.vendor_id,
        )
    }
}

/// Who-Is service handler.
pub struct WhoIsHandler {
    device_instance: u32,
    max_apdu_length: u16,
    segmentation: SegmentationSupport,
    vendor_id: u16,
}

impl WhoIsHandler {
    /// Create a new Who-Is handler.
    pub fn new(
        device_instance: u32,
        max_apdu_length: u16,
        segmentation: SegmentationSupport,
        vendor_id: u16,
    ) -> Self {
        Self {
            device_instance,
            max_apdu_length,
            segmentation,
            vendor_id,
        }
    }
}

impl UnconfirmedServiceHandler for WhoIsHandler {
    fn service_choice(&self) -> UnconfirmedService {
        UnconfirmedService::WhoIs
    }

    fn handle(&self, data: &[u8], _ctx: &ServiceContext) -> ServiceResult {
        let request = WhoIsRequest::decode(data);

        if request.matches(self.device_instance) {
            let i_am = IAmResponse::new(
                self.device_instance,
                self.max_apdu_length,
                self.segmentation,
                self.vendor_id,
            );
            ServiceResult::Broadcast(i_am.encode())
        } else {
            ServiceResult::NoResponse
        }
    }

    fn name(&self) -> &'static str {
        "Who-Is"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_who_is_all() {
        let request = WhoIsRequest::all();

        assert!(request.matches(0));
        assert!(request.matches(1000));
        assert!(request.matches(4194302));
    }

    #[test]
    fn test_who_is_range() {
        let request = WhoIsRequest::range(100, 200);

        assert!(!request.matches(99));
        assert!(request.matches(100));
        assert!(request.matches(150));
        assert!(request.matches(200));
        assert!(!request.matches(201));
    }

    #[test]
    fn test_who_is_encode_decode() {
        let request = WhoIsRequest::range(100, 200);
        let encoded = request.encode();
        let decoded = WhoIsRequest::decode(&encoded);

        assert_eq!(decoded.device_instance_low, Some(100));
        assert_eq!(decoded.device_instance_high, Some(200));
    }

    #[test]
    fn test_i_am_encode_decode() {
        let i_am = IAmResponse::new(1234, 1476, SegmentationSupport::None, 999);
        let encoded = i_am.encode();
        let decoded = IAmResponse::decode(&encoded).unwrap();

        assert_eq!(decoded.device_identifier.instance, 1234);
        assert_eq!(decoded.max_apdu_length_accepted, 1476);
        assert_eq!(decoded.vendor_id, 999);
    }

    #[test]
    fn test_discovery_service() {
        let service = DiscoveryService::new(1234, 999);

        // Should match all devices
        let request = WhoIsRequest::all();
        let response = service.handle_who_is(&request);
        assert!(response.is_some());

        // Should match specific range
        let request = WhoIsRequest::range(1000, 2000);
        let response = service.handle_who_is(&request);
        assert!(response.is_some());

        // Should not match different range
        let request = WhoIsRequest::range(0, 100);
        let response = service.handle_who_is(&request);
        assert!(response.is_none());
    }
}
