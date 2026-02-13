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
pub mod builder;
pub mod cache;
pub mod classes;
pub mod prefetch;
pub mod reference;
pub mod store;
pub mod variables;

pub use base::{Node, NodeBase, NodeClass, QualifiedName, LocalizedText};
pub use batch::{
    BatchNodeCreator, BatchConfig, BatchProgress, ProgressCallback,
    VariableTemplate, ObjectTemplate, ValueGeneratorType,
};
pub use builder::{NodeBuilder, VariableBuilder, ObjectBuilder, FolderBuilder, BatchVariableBuilder, AddToAddressSpace};
pub use cache::{NodeCache, NodeCacheConfig, CacheStats};
pub use classes::{
    ObjectNode, VariableNode, MethodNode,
    ObjectTypeNode, VariableTypeNode, ReferenceTypeNode, DataTypeNode, ViewNode,
};
pub use prefetch::{
    NodePrefetcher, PrefetchConfig, PrefetchStats,
    AsyncPrefetchWorker, PrefetchingAddressSpace,
};
pub use reference::{
    Reference, ReferenceDescription, ReferenceDirection, ReferenceTypeId,
    BrowseDirection, BrowseResult,
};
pub use store::{AddressSpace, AddressSpaceConfig, NodeStoreStats, RelativePathElement, BrowsePathResult, BrowsePathTarget};
pub use variables::{VariableFactory, AnalogVariable, DiscreteVariable};
