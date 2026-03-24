//! Fault registry for managing fault instances.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

use crate::error::{ChaosError, ChaosResult};
use crate::fault::{BoxedFault, Fault, FaultCategory, FaultSeverity};

// =============================================================================
// Fault Filter
// =============================================================================

/// Filter for querying faults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FaultFilter {
    /// Filter by category.
    pub category: Option<FaultCategory>,

    /// Filter by severity.
    pub min_severity: Option<FaultSeverity>,

    /// Filter by enabled status.
    pub enabled_only: bool,

    /// Filter by target pattern.
    pub target: Option<String>,

    /// Filter by tags.
    pub tags: Vec<String>,
}

impl FaultFilter {
    /// Create a new empty filter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by category.
    pub fn category(mut self, category: FaultCategory) -> Self {
        self.category = Some(category);
        self
    }

    /// Filter by minimum severity.
    pub fn min_severity(mut self, severity: FaultSeverity) -> Self {
        self.min_severity = Some(severity);
        self
    }

    /// Filter to enabled faults only.
    pub fn enabled(mut self) -> Self {
        self.enabled_only = true;
        self
    }

    /// Filter by target.
    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// Filter by tag.
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Check if a fault matches the filter.
    pub fn matches(&self, fault: &dyn Fault) -> bool {
        let metadata = fault.metadata();

        // Check category
        if let Some(cat) = &self.category {
            if metadata.category != *cat {
                return false;
            }
        }

        // Check severity
        if let Some(min) = &self.min_severity {
            if metadata.severity < *min {
                return false;
            }
        }

        // Check enabled
        if self.enabled_only && !metadata.enabled {
            return false;
        }

        // Check target
        if let Some(target) = &self.target {
            if !metadata.matches_target(target) {
                return false;
            }
        }

        // Check tags
        if !self.tags.is_empty() {
            let has_all_tags = self.tags.iter().all(|t| metadata.tags.contains(t));
            if !has_all_tags {
                return false;
            }
        }

        true
    }
}

// =============================================================================
// Fault Entry
// =============================================================================

/// Entry in the fault registry.
#[derive(Debug)]
pub struct FaultEntry {
    /// The fault instance.
    pub fault: BoxedFault,

    /// Associated targets (devices/protocols this fault is active for).
    pub active_targets: Vec<String>,

    /// Whether the fault is globally active.
    pub globally_active: bool,
}

impl FaultEntry {
    /// Create a new fault entry.
    pub fn new(fault: BoxedFault) -> Self {
        Self {
            fault,
            active_targets: Vec::new(),
            globally_active: false,
        }
    }

    /// Check if active for a target.
    pub fn is_active_for(&self, target: &str) -> bool {
        self.globally_active
            || self.active_targets.iter().any(|t| {
                if t.contains('*') {
                    // Glob matching
                    glob_match(t, target)
                } else {
                    t == target
                }
            })
    }

    /// Activate for a target.
    pub fn activate_for(&mut self, target: impl Into<String>) {
        let target = target.into();
        if !self.active_targets.contains(&target) {
            self.active_targets.push(target);
        }
    }

    /// Deactivate for a target.
    pub fn deactivate_for(&mut self, target: &str) {
        self.active_targets.retain(|t| t != target);
    }

    /// Activate globally.
    pub fn activate_globally(&mut self) {
        self.globally_active = true;
    }

    /// Deactivate globally.
    pub fn deactivate_globally(&mut self) {
        self.globally_active = false;
        self.active_targets.clear();
    }
}

/// Simple glob matching.
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if pattern.starts_with('*') && pattern.ends_with('*') {
        let middle = &pattern[1..pattern.len() - 1];
        return text.contains(middle);
    }

    if pattern.starts_with('*') {
        return text.ends_with(&pattern[1..]);
    }

    if pattern.ends_with('*') {
        return text.starts_with(&pattern[..pattern.len() - 1]);
    }

    pattern == text
}

// =============================================================================
// Fault Registry
// =============================================================================

/// Registry for managing fault instances.
///
/// Provides thread-safe storage and lookup of faults.
///
/// # Example
///
/// ```rust,ignore
/// use mabi_chaos::FaultRegistry;
///
/// let registry = FaultRegistry::new();
///
/// // Register a fault
/// registry.register("latency", latency_fault)?;
///
/// // Activate for specific target
/// registry.activate("latency", "device-001")?;
///
/// // Get active faults for a target
/// let faults = registry.active_for("device-001");
/// ```
#[derive(Debug)]
pub struct FaultRegistry {
    faults: DashMap<String, FaultEntry>,
}

impl FaultRegistry {
    /// Create a new fault registry.
    pub fn new() -> Self {
        Self {
            faults: DashMap::new(),
        }
    }

    /// Register a fault.
    pub fn register(&self, id: impl Into<String>, fault: BoxedFault) -> ChaosResult<()> {
        let id = id.into();
        if self.faults.contains_key(&id) {
            return Err(ChaosError::FaultAlreadyActive {
                fault_id: id,
                target_id: "registry".to_string(),
            });
        }
        self.faults.insert(id, FaultEntry::new(fault));
        Ok(())
    }

    /// Unregister a fault.
    pub fn unregister(&self, id: &str) -> ChaosResult<BoxedFault> {
        self.faults
            .remove(id)
            .map(|(_, entry)| entry.fault)
            .ok_or_else(|| ChaosError::fault_not_found(id))
    }

    /// Get a fault by ID.
    pub fn get(&self, id: &str) -> Option<dashmap::mapref::one::Ref<'_, String, FaultEntry>> {
        self.faults.get(id)
    }

    /// Get a mutable reference to a fault.
    pub fn get_mut(
        &self,
        id: &str,
    ) -> Option<dashmap::mapref::one::RefMut<'_, String, FaultEntry>> {
        self.faults.get_mut(id)
    }

    /// Check if a fault exists.
    pub fn contains(&self, id: &str) -> bool {
        self.faults.contains_key(id)
    }

    /// Activate a fault for a target.
    pub fn activate(&self, fault_id: &str, target: impl Into<String>) -> ChaosResult<()> {
        let target = target.into();
        let mut entry = self
            .faults
            .get_mut(fault_id)
            .ok_or_else(|| ChaosError::fault_not_found(fault_id))?;
        entry.activate_for(target);
        entry.fault.enable();
        Ok(())
    }

    /// Deactivate a fault for a target.
    pub fn deactivate(&self, fault_id: &str, target: &str) -> ChaosResult<()> {
        let mut entry = self
            .faults
            .get_mut(fault_id)
            .ok_or_else(|| ChaosError::fault_not_found(fault_id))?;
        entry.deactivate_for(target);
        Ok(())
    }

    /// Activate a fault globally.
    pub fn activate_globally(&self, fault_id: &str) -> ChaosResult<()> {
        let mut entry = self
            .faults
            .get_mut(fault_id)
            .ok_or_else(|| ChaosError::fault_not_found(fault_id))?;
        entry.activate_globally();
        entry.fault.enable();
        Ok(())
    }

    /// Deactivate a fault globally.
    pub fn deactivate_globally(&self, fault_id: &str) -> ChaosResult<()> {
        let mut entry = self
            .faults
            .get_mut(fault_id)
            .ok_or_else(|| ChaosError::fault_not_found(fault_id))?;
        entry.deactivate_globally();
        entry.fault.disable();
        Ok(())
    }

    /// Get all faults active for a target.
    pub fn active_for(&self, target: &str) -> Vec<String> {
        self.faults
            .iter()
            .filter(|entry| entry.is_active_for(target))
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Get all fault IDs.
    pub fn ids(&self) -> Vec<String> {
        self.faults.iter().map(|e| e.key().clone()).collect()
    }

    /// Get all faults matching a filter.
    pub fn filter(&self, filter: &FaultFilter) -> Vec<String> {
        self.faults
            .iter()
            .filter(|entry| filter.matches(entry.fault.as_ref()))
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Get fault count.
    pub fn len(&self) -> usize {
        self.faults.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.faults.is_empty()
    }

    /// Clear all faults.
    pub fn clear(&self) {
        self.faults.clear();
    }

    /// Reset all faults.
    pub fn reset_all(&self) {
        for mut entry in self.faults.iter_mut() {
            entry.fault.reset();
            entry.active_targets.clear();
            entry.globally_active = false;
        }
    }
}

impl Default for FaultRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::NetworkLatencyFault;

    fn create_test_fault(id: &str) -> BoxedFault {
        Box::new(NetworkLatencyFault::builder().id(id).base_ms(100).build())
    }

    #[test]
    fn test_register_and_get() {
        let registry = FaultRegistry::new();
        let fault = create_test_fault("test-001");

        registry.register("test-001", fault).unwrap();

        assert!(registry.contains("test-001"));
        assert!(!registry.contains("test-002"));
    }

    #[test]
    fn test_duplicate_registration() {
        let registry = FaultRegistry::new();

        registry
            .register("test", create_test_fault("test"))
            .unwrap();
        let result = registry.register("test", create_test_fault("test"));

        assert!(result.is_err());
    }

    #[test]
    fn test_activation() {
        let registry = FaultRegistry::new();
        registry
            .register("test", create_test_fault("test"))
            .unwrap();

        // Not active initially
        assert!(registry.active_for("device-001").is_empty());

        // Activate for specific target
        registry.activate("test", "device-001").unwrap();
        assert!(registry
            .active_for("device-001")
            .contains(&"test".to_string()));

        // Not active for other targets
        assert!(registry.active_for("device-002").is_empty());
    }

    #[test]
    fn test_global_activation() {
        let registry = FaultRegistry::new();
        registry
            .register("test", create_test_fault("test"))
            .unwrap();

        registry.activate_globally("test").unwrap();

        assert!(registry
            .active_for("any-device")
            .contains(&"test".to_string()));
        assert!(registry
            .active_for("another-device")
            .contains(&"test".to_string()));
    }

    #[test]
    fn test_glob_matching() {
        assert!(glob_match("device-*", "device-001"));
        assert!(glob_match("*-sensor", "temp-sensor"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*bus*", "modbus-server"));
        assert!(!glob_match("device-*", "server-001"));
    }

    #[test]
    fn test_filter() {
        let registry = FaultRegistry::new();
        registry
            .register("net-001", create_test_fault("net-001"))
            .unwrap();
        registry
            .register("net-002", create_test_fault("net-002"))
            .unwrap();

        let filter = FaultFilter::new().category(FaultCategory::Network);
        let results = registry.filter(&filter);

        assert_eq!(results.len(), 2);
    }
}
