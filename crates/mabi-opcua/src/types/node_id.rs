//! OPC UA NodeId implementation.
//!
//! NodeId is the fundamental identifier for all nodes in the OPC UA address space.
//! This implementation supports numeric, string, GUID, and opaque (ByteString) identifiers.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Counter for generating unique numeric node IDs.
static NODE_ID_COUNTER: AtomicU32 = AtomicU32::new(1000);

/// The type of node identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeIdType {
    /// Numeric identifier (most common and efficient).
    Numeric(u32),
    /// String identifier.
    String(String),
    /// GUID identifier.
    Guid(Uuid),
    /// ByteString (opaque) identifier.
    ByteString(Vec<u8>),
}

impl fmt::Display for NodeIdType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Numeric(n) => write!(f, "i={}", n),
            Self::String(s) => write!(f, "s={}", s),
            Self::Guid(g) => write!(f, "g={}", g),
            Self::ByteString(b) => write!(f, "b={}", hex::encode(b)),
        }
    }
}

/// OPC UA Node ID.
///
/// A NodeId uniquely identifies a node within a namespace.
/// The namespace index determines which namespace the identifier belongs to.
///
/// # Examples
///
/// ```
/// use mabi_opcua::types::NodeId;
///
/// // Create a numeric node ID
/// let node_id = NodeId::numeric(2, 1001);
/// assert_eq!(node_id.namespace(), 2);
///
/// // Create a string node ID
/// let node_id = NodeId::string(2, "Temperature");
///
/// // Parse from string
/// let node_id: NodeId = "ns=2;i=1001".parse().unwrap();
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeId {
    /// Namespace index.
    namespace: u16,
    /// The identifier.
    identifier: NodeIdType,
}

impl NodeId {
    /// Create a new numeric node ID.
    pub fn numeric(namespace: u16, identifier: u32) -> Self {
        Self {
            namespace,
            identifier: NodeIdType::Numeric(identifier),
        }
    }

    /// Create a new string node ID.
    pub fn string(namespace: u16, identifier: impl Into<String>) -> Self {
        Self {
            namespace,
            identifier: NodeIdType::String(identifier.into()),
        }
    }

    /// Create a new GUID node ID.
    pub fn guid(namespace: u16, identifier: Uuid) -> Self {
        Self {
            namespace,
            identifier: NodeIdType::Guid(identifier),
        }
    }

    /// Create a new ByteString node ID.
    pub fn byte_string(namespace: u16, identifier: impl Into<Vec<u8>>) -> Self {
        Self {
            namespace,
            identifier: NodeIdType::ByteString(identifier.into()),
        }
    }

    /// Generate a new unique numeric node ID in the specified namespace.
    pub fn next_numeric(namespace: u16) -> Self {
        let id = NODE_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        Self::numeric(namespace, id)
    }

    /// Get the namespace index.
    pub fn namespace(&self) -> u16 {
        self.namespace
    }

    /// Get the identifier.
    pub fn identifier(&self) -> &NodeIdType {
        &self.identifier
    }

    /// Check if this is a numeric identifier.
    pub fn is_numeric(&self) -> bool {
        matches!(self.identifier, NodeIdType::Numeric(_))
    }

    /// Check if this is a string identifier.
    pub fn is_string(&self) -> bool {
        matches!(self.identifier, NodeIdType::String(_))
    }

    /// Get as numeric value if applicable.
    pub fn as_numeric(&self) -> Option<u32> {
        match &self.identifier {
            NodeIdType::Numeric(n) => Some(*n),
            _ => None,
        }
    }

    /// Get as string value if applicable.
    pub fn as_string(&self) -> Option<&str> {
        match &self.identifier {
            NodeIdType::String(s) => Some(s),
            _ => None,
        }
    }

    /// Check if this is a null node ID.
    pub fn is_null(&self) -> bool {
        match &self.identifier {
            NodeIdType::Numeric(n) => self.namespace == 0 && *n == 0,
            NodeIdType::String(s) => s.is_empty(),
            NodeIdType::Guid(g) => g.is_nil(),
            NodeIdType::ByteString(b) => b.is_empty(),
        }
    }

    /// Create a null node ID.
    pub fn null() -> Self {
        Self::numeric(0, 0)
    }

    /// Standard OPC UA root folder node ID (namespace 0, identifier 84).
    pub fn root_folder() -> Self {
        Self::numeric(0, 84)
    }

    /// Standard OPC UA objects folder node ID (namespace 0, identifier 85).
    pub fn objects_folder() -> Self {
        Self::numeric(0, 85)
    }

    /// Standard OPC UA types folder node ID (namespace 0, identifier 86).
    pub fn types_folder() -> Self {
        Self::numeric(0, 86)
    }

    /// Standard OPC UA views folder node ID (namespace 0, identifier 87).
    pub fn views_folder() -> Self {
        Self::numeric(0, 87)
    }

    /// Standard OPC UA server node ID (namespace 0, identifier 2253).
    pub fn server() -> Self {
        Self::numeric(0, 2253)
    }
}

impl PartialEq for NodeId {
    fn eq(&self, other: &Self) -> bool {
        self.namespace == other.namespace && self.identifier == other.identifier
    }
}

impl Eq for NodeId {}

impl Hash for NodeId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.namespace.hash(state);
        self.identifier.hash(state);
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.namespace == 0 {
            write!(f, "{}", self.identifier)
        } else {
            write!(f, "ns={};{}", self.namespace, self.identifier)
        }
    }
}

impl std::str::FromStr for NodeId {
    type Err = NodeIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // Parse namespace
        let (namespace, rest) = if s.starts_with("ns=") {
            let parts: Vec<&str> = s.splitn(2, ';').collect();
            if parts.len() != 2 {
                return Err(NodeIdParseError::InvalidFormat);
            }
            let ns_str = parts[0].strip_prefix("ns=").unwrap();
            let ns: u16 = ns_str.parse().map_err(|_| NodeIdParseError::InvalidNamespace)?;
            (ns, parts[1])
        } else {
            (0, s)
        };

        // Parse identifier
        let identifier = if let Some(id_str) = rest.strip_prefix("i=") {
            let id: u32 = id_str.parse().map_err(|_| NodeIdParseError::InvalidIdentifier)?;
            NodeIdType::Numeric(id)
        } else if let Some(id_str) = rest.strip_prefix("s=") {
            NodeIdType::String(id_str.to_string())
        } else if let Some(id_str) = rest.strip_prefix("g=") {
            let uuid = Uuid::parse_str(id_str).map_err(|_| NodeIdParseError::InvalidIdentifier)?;
            NodeIdType::Guid(uuid)
        } else if let Some(id_str) = rest.strip_prefix("b=") {
            let bytes = hex::decode(id_str).map_err(|_| NodeIdParseError::InvalidIdentifier)?;
            NodeIdType::ByteString(bytes)
        } else {
            // Try to parse as numeric without prefix
            if let Ok(id) = rest.parse::<u32>() {
                NodeIdType::Numeric(id)
            } else {
                return Err(NodeIdParseError::InvalidFormat);
            }
        };

        Ok(Self { namespace, identifier })
    }
}

/// Error type for NodeId parsing.
#[derive(Debug, Clone, thiserror::Error)]
pub enum NodeIdParseError {
    #[error("Invalid node ID format")]
    InvalidFormat,
    #[error("Invalid namespace index")]
    InvalidNamespace,
    #[error("Invalid identifier")]
    InvalidIdentifier,
}

/// Expanded Node ID (includes server index and namespace URI).
///
/// Used for node IDs that need to be resolved across servers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExpandedNodeId {
    /// The base node ID.
    pub node_id: NodeId,
    /// Server index (0 = local server).
    pub server_index: u32,
    /// Namespace URI (optional, overrides namespace index if present).
    pub namespace_uri: Option<String>,
}

impl ExpandedNodeId {
    /// Create from a NodeId.
    pub fn from_node_id(node_id: NodeId) -> Self {
        Self {
            node_id,
            server_index: 0,
            namespace_uri: None,
        }
    }

    /// Create with a namespace URI.
    pub fn with_namespace_uri(node_id: NodeId, namespace_uri: impl Into<String>) -> Self {
        Self {
            node_id,
            server_index: 0,
            namespace_uri: Some(namespace_uri.into()),
        }
    }

    /// Check if this is a null expanded node ID.
    pub fn is_null(&self) -> bool {
        self.node_id.is_null() && self.server_index == 0 && self.namespace_uri.is_none()
    }

    /// Get the inner NodeId.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }
}

impl From<NodeId> for ExpandedNodeId {
    fn from(node_id: NodeId) -> Self {
        Self::from_node_id(node_id)
    }
}

impl fmt::Display for ExpandedNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.server_index > 0 {
            write!(f, "svr={};", self.server_index)?;
        }
        if let Some(ref uri) = self.namespace_uri {
            write!(f, "nsu={};{}", uri, self.node_id.identifier())
        } else {
            write!(f, "{}", self.node_id)
        }
    }
}

// Simple hex encoding/decoding for ByteString
mod hex {
    pub fn encode(data: &[u8]) -> String {
        data.iter().map(|b| format!("{:02x}", b)).collect()
    }

    pub fn decode(s: &str) -> Result<Vec<u8>, ()> {
        if s.len() % 2 != 0 {
            return Err(());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numeric_node_id() {
        let node_id = NodeId::numeric(2, 1001);
        assert_eq!(node_id.namespace(), 2);
        assert_eq!(node_id.as_numeric(), Some(1001));
        assert!(node_id.is_numeric());
    }

    #[test]
    fn test_string_node_id() {
        let node_id = NodeId::string(2, "Temperature");
        assert_eq!(node_id.namespace(), 2);
        assert_eq!(node_id.as_string(), Some("Temperature"));
        assert!(node_id.is_string());
    }

    #[test]
    fn test_node_id_display() {
        let node_id = NodeId::numeric(0, 84);
        assert_eq!(node_id.to_string(), "i=84");

        let node_id = NodeId::numeric(2, 1001);
        assert_eq!(node_id.to_string(), "ns=2;i=1001");

        let node_id = NodeId::string(2, "Test");
        assert_eq!(node_id.to_string(), "ns=2;s=Test");
    }

    #[test]
    fn test_node_id_parse() {
        let node_id: NodeId = "i=84".parse().unwrap();
        assert_eq!(node_id, NodeId::numeric(0, 84));

        let node_id: NodeId = "ns=2;i=1001".parse().unwrap();
        assert_eq!(node_id, NodeId::numeric(2, 1001));

        let node_id: NodeId = "ns=2;s=Temperature".parse().unwrap();
        assert_eq!(node_id, NodeId::string(2, "Temperature"));
    }

    #[test]
    fn test_null_node_id() {
        let node_id = NodeId::null();
        assert!(node_id.is_null());

        let node_id = NodeId::numeric(0, 1);
        assert!(!node_id.is_null());
    }

    #[test]
    fn test_standard_node_ids() {
        assert_eq!(NodeId::root_folder(), NodeId::numeric(0, 84));
        assert_eq!(NodeId::objects_folder(), NodeId::numeric(0, 85));
        assert_eq!(NodeId::types_folder(), NodeId::numeric(0, 86));
    }

    #[test]
    fn test_next_numeric() {
        let id1 = NodeId::next_numeric(2);
        let id2 = NodeId::next_numeric(2);
        assert_ne!(id1, id2);
    }
}
