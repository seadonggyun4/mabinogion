use std::sync::Arc;

use dashmap::DashMap;
use tracing::debug;

use crate::types::{NodeId, StatusCode, Variant};

pub(crate) type MethodCallback =
    Arc<dyn Fn(&[Variant]) -> Result<Vec<Variant>, StatusCode> + Send + Sync>;

pub(crate) struct MethodRegistry {
    callbacks: DashMap<NodeId, MethodCallback>,
}

impl MethodRegistry {
    pub(crate) fn new() -> Self {
        Self {
            callbacks: DashMap::new(),
        }
    }

    pub(crate) fn register(&self, method_id: NodeId, callback: MethodCallback) {
        debug!(method_id = %method_id, "Registered method callback");
        self.callbacks.insert(method_id, callback);
    }

    pub(crate) fn get(&self, method_id: &NodeId) -> Option<MethodCallback> {
        self.callbacks
            .get(method_id)
            .map(|value| value.value().clone())
    }

    pub(crate) fn contains(&self, method_id: &NodeId) -> bool {
        self.callbacks.contains_key(method_id)
    }

    pub(crate) fn len(&self) -> usize {
        self.callbacks.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.callbacks.is_empty()
    }
}

impl Default for MethodRegistry {
    fn default() -> Self {
        Self::new()
    }
}
