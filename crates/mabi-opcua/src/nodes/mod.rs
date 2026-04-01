//! OPC UA Node system.
//!
//! This module provides the node hierarchy and management for OPC UA address space.
//!
//! ## Node Classes
//!
//! OPC UA defines several node classes:
//! - **Object**: Container for variables and methods
//! - **Variable**: Holds a value that can be read/written
//! - **Method**: Callable function on an object
//! - **ObjectType**: Type definition for objects
//! - **VariableType**: Type definition for variables
//! - **ReferenceType**: Defines relationship types between nodes
//! - **DataType**: Defines data types
//! - **View**: Subset of the address space
//!
//! ## Architecture
//!
//! ```text
//! AddressSpace
//!     ├── NodeStore (DashMap<NodeId, Node>)
//!     ├── ReferenceStore (references between nodes)
//!     └── NodeBuilder (fluent API for node creation)
//! ```

pub mod base;
pub mod batch;
pub(crate) mod builder;
pub mod cache;
pub mod classes;
pub(crate) mod prefetch;
pub mod reference;
pub mod store;
pub(crate) mod variables;

pub use base::{LocalizedText, Node, NodeBase, NodeClass, QualifiedName};
pub use batch::{
    BatchConfig, BatchNodeCreator, BatchProgress, ObjectTemplate, ProgressCallback,
    ValueGeneratorType, VariableTemplate,
};
pub use cache::{CacheStats, NodeCache, NodeCacheConfig};
pub use classes::{
    DataTypeNode, MethodNode, ObjectNode, ObjectTypeNode, ReferenceTypeNode, VariableNode,
    VariableTypeNode, ViewNode,
};
pub use prefetch::{AsyncPrefetchWorker, NodePrefetcher, PrefetchConfig, PrefetchStats};
pub use reference::{
    BrowseDirection, BrowseResult, Reference, ReferenceDescription, ReferenceDirection,
    ReferenceTypeId,
};
pub use store::{
    AddressSpace, AddressSpaceConfig, BrowsePathResult, BrowsePathTarget, NodeStoreStats,
    RelativePathElement,
};
pub use variables::{AnalogVariable, DiscreteVariable};
