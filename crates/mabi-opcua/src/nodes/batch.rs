//! Batch Node Creation for High-Performance Address Space Population.
//!
//! This module provides efficient mechanisms for creating large numbers of nodes
//! (10,000+) with support for templates, parallel processing, and progress tracking.
//!
//! ## Features
//!
//! - **Template-based creation**: Define node templates and instantiate many nodes
//! - **Parallel processing**: Multi-threaded node creation for large batches
//! - **Progress tracking**: Monitor batch creation progress
//! - **Memory efficiency**: Streaming creation to avoid memory spikes
//! - **Hierarchical creation**: Create entire subtrees with single operation

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::builder::{NodeBuilder, ObjectBuilder, VariableBuilder};
use super::classes::VariableNode;
use super::reference::Reference;
use super::store::AddressSpace;
use crate::error::OpcUaResult;
use crate::types::{variant::DataTypeId, NodeId, Variant};

/// Configuration for batch node creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchConfig {
    /// Number of parallel workers for node creation.
    pub parallel_workers: usize,
    /// Chunk size for parallel processing.
    pub chunk_size: usize,
    /// Whether to validate node IDs before insertion.
    pub validate_node_ids: bool,
    /// Whether to create parent references automatically.
    pub auto_references: bool,
    /// Progress reporting interval (number of nodes).
    pub progress_interval: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            parallel_workers: num_cpus::get().max(1),
            chunk_size: 1000,
            validate_node_ids: true,
            auto_references: true,
            progress_interval: 1000,
        }
    }
}

impl BatchConfig {
    /// Create config for small batches (< 1,000 nodes).
    pub fn small() -> Self {
        Self {
            parallel_workers: 1,
            chunk_size: 100,
            progress_interval: 100,
            ..Default::default()
        }
    }

    /// Create config for medium batches (1,000 - 10,000 nodes).
    pub fn medium() -> Self {
        Self {
            parallel_workers: 4,
            chunk_size: 500,
            progress_interval: 500,
            ..Default::default()
        }
    }

    /// Create config for large batches (10,000+ nodes).
    pub fn large() -> Self {
        Self {
            parallel_workers: num_cpus::get().max(4),
            chunk_size: 2000,
            progress_interval: 2000,
            ..Default::default()
        }
    }
}

/// Progress information for batch operations.
#[derive(Debug, Clone)]
pub struct BatchProgress {
    /// Total number of nodes to create.
    pub total: usize,
    /// Number of nodes created so far.
    pub created: usize,
    /// Number of nodes that failed to create.
    pub failed: usize,
    /// Percentage complete (0.0 - 100.0).
    pub percent_complete: f64,
}

/// Callback type for progress reporting.
pub type ProgressCallback = Arc<dyn Fn(BatchProgress) + Send + Sync>;

/// Template for creating variable nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableTemplate {
    /// Base name (will be suffixed with index).
    pub name_prefix: String,
    /// Data type.
    pub data_type: DataTypeId,
    /// Default value generator type.
    pub value_type: ValueGeneratorType,
    /// Whether writable.
    pub writable: bool,
    /// Whether historizing.
    pub historizing: bool,
    /// Minimum sampling interval.
    pub sampling_interval: f64,
}

impl Default for VariableTemplate {
    fn default() -> Self {
        Self {
            name_prefix: "Variable".to_string(),
            data_type: DataTypeId::Double,
            value_type: ValueGeneratorType::Zero,
            writable: false,
            historizing: false,
            sampling_interval: 0.0,
        }
    }
}

impl VariableTemplate {
    /// Create a new template.
    pub fn new(name_prefix: impl Into<String>, data_type: DataTypeId) -> Self {
        Self {
            name_prefix: name_prefix.into(),
            data_type,
            ..Default::default()
        }
    }

    /// Set the value generator type.
    pub fn with_value_type(mut self, value_type: ValueGeneratorType) -> Self {
        self.value_type = value_type;
        self
    }

    /// Make variables writable.
    pub fn writable(mut self) -> Self {
        self.writable = true;
        self
    }

    /// Enable historizing.
    pub fn historizing(mut self) -> Self {
        self.historizing = true;
        self
    }

    /// Set sampling interval.
    pub fn sampling_interval(mut self, interval: f64) -> Self {
        self.sampling_interval = interval;
        self
    }
}

/// Type of value generator for batch creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValueGeneratorType {
    /// All zeros.
    Zero,
    /// Sequential values (0, 1, 2, ...).
    Sequential,
    /// Random values within a range.
    Random { min: f64, max: f64 },
    /// Fixed value.
    Fixed(f64),
    /// Sine wave pattern.
    SineWave { amplitude: f64, period: usize },
    /// Custom function index.
    Custom(String),
}

impl ValueGeneratorType {
    /// Generate a value for the given index.
    pub fn generate(&self, index: usize) -> Variant {
        match self {
            Self::Zero => Variant::Double(0.0),
            Self::Sequential => Variant::Double(index as f64),
            Self::Random { min, max } => {
                // Simple pseudo-random based on index
                let factor = ((index * 7919 + 104729) % 10000) as f64 / 10000.0;
                Variant::Double(min + factor * (max - min))
            }
            Self::Fixed(v) => Variant::Double(*v),
            Self::SineWave { amplitude, period } => {
                let angle = (index as f64 / *period as f64) * 2.0 * std::f64::consts::PI;
                Variant::Double(amplitude * angle.sin())
            }
            Self::Custom(_) => Variant::Double(0.0), // Placeholder for custom generators
        }
    }
}

/// Template for creating object nodes with child variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectTemplate {
    /// Base name for the object.
    pub name_prefix: String,
    /// Variable templates for child nodes.
    pub variables: Vec<VariableTemplate>,
    /// Nested object templates.
    pub children: Vec<ObjectTemplate>,
}

impl ObjectTemplate {
    /// Create a new object template.
    pub fn new(name_prefix: impl Into<String>) -> Self {
        Self {
            name_prefix: name_prefix.into(),
            variables: Vec::new(),
            children: Vec::new(),
        }
    }

    /// Add a variable template.
    pub fn with_variable(mut self, template: VariableTemplate) -> Self {
        self.variables.push(template);
        self
    }

    /// Add multiple variable templates.
    pub fn with_variables(mut self, templates: Vec<VariableTemplate>) -> Self {
        self.variables.extend(templates);
        self
    }

    /// Add a child object template.
    pub fn with_child(mut self, child: ObjectTemplate) -> Self {
        self.children.push(child);
        self
    }

    /// Count total nodes in this template (recursive).
    pub fn total_node_count(&self) -> usize {
        1 + self.variables.len()
            + self
                .children
                .iter()
                .map(|c| c.total_node_count())
                .sum::<usize>()
    }
}

/// Batch node creator with support for templates and parallel processing.
pub struct BatchNodeCreator {
    config: BatchConfig,
    progress_callback: Option<ProgressCallback>,
    created_count: Arc<AtomicUsize>,
    failed_count: Arc<AtomicUsize>,
}

impl std::fmt::Debug for BatchNodeCreator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BatchNodeCreator")
            .field("config", &self.config)
            .field(
                "progress_callback",
                &self.progress_callback.as_ref().map(|_| "<callback>"),
            )
            .field("created_count", &self.created_count)
            .field("failed_count", &self.failed_count)
            .finish()
    }
}

impl BatchNodeCreator {
    /// Create a new batch node creator.
    pub fn new(config: BatchConfig) -> Self {
        Self {
            config,
            progress_callback: None,
            created_count: Arc::new(AtomicUsize::new(0)),
            failed_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Set progress callback.
    pub fn with_progress_callback(mut self, callback: ProgressCallback) -> Self {
        self.progress_callback = Some(callback);
        self
    }

    /// Create variables from a template.
    pub fn create_variables_from_template(
        &self,
        address_space: &AddressSpace,
        parent_id: &NodeId,
        template: &VariableTemplate,
        namespace: u16,
        start_id: u32,
        count: usize,
    ) -> OpcUaResult<Vec<NodeId>> {
        self.reset_counters();
        let total = count;

        // Generate variable specifications
        let specs: Vec<_> = (0..count)
            .map(|i| {
                let node_id = NodeId::numeric(namespace, start_id + i as u32);
                let name = format!("{}_{}", template.name_prefix, i);
                let value = template.value_type.generate(i);
                (node_id, name, value)
            })
            .collect();

        // Process in chunks (potentially parallel)
        let chunk_size = self.config.chunk_size;
        let mut all_node_ids = Vec::with_capacity(count);

        for (_chunk_idx, chunk) in specs.chunks(chunk_size).enumerate() {
            let mut chunk_nodes = Vec::with_capacity(chunk.len());

            for (node_id, name, value) in chunk {
                let mut builder = VariableBuilder::new(node_id.clone())
                    .browse_name(namespace, name)
                    .display_name(name)
                    .data_type(template.data_type)
                    .value(value.clone())
                    .sampling_interval(template.sampling_interval);

                if template.writable {
                    builder = builder.writable();
                }
                if template.historizing {
                    builder = builder.historizing();
                }

                chunk_nodes.push(builder.build());
            }

            // Insert nodes into address space
            for node in chunk_nodes {
                let node_id = node.base.node_id.clone();
                if address_space.insert_node(node) {
                    if self.config.auto_references {
                        address_space.add_reference(Reference::has_component(
                            parent_id.clone(),
                            node_id.clone(),
                        ));
                    }
                    all_node_ids.push(node_id);
                    self.created_count.fetch_add(1, Ordering::Relaxed);
                } else {
                    self.failed_count.fetch_add(1, Ordering::Relaxed);
                }
            }

            // Report progress
            self.report_progress(total);
        }

        Ok(all_node_ids)
    }

    /// Create variables in parallel using rayon.
    pub fn create_variables_parallel(
        &self,
        address_space: &AddressSpace,
        parent_id: &NodeId,
        template: &VariableTemplate,
        namespace: u16,
        start_id: u32,
        count: usize,
    ) -> OpcUaResult<Vec<NodeId>> {
        self.reset_counters();
        let total = count;

        // Create all variable nodes in parallel
        let nodes: Vec<VariableNode> = (0..count)
            .into_par_iter()
            .map(|i| {
                let node_id = NodeId::numeric(namespace, start_id + i as u32);
                let name = format!("{}_{}", template.name_prefix, i);
                let value = template.value_type.generate(i);

                let mut builder = VariableBuilder::new(node_id)
                    .browse_name(namespace, &name)
                    .display_name(&name)
                    .data_type(template.data_type)
                    .value(value)
                    .sampling_interval(template.sampling_interval);

                if template.writable {
                    builder = builder.writable();
                }
                if template.historizing {
                    builder = builder.historizing();
                }

                builder.build()
            })
            .collect();

        // Insert sequentially into address space (thread-safe via DashMap)
        let mut node_ids = Vec::with_capacity(nodes.len());
        for node in nodes {
            let node_id = node.base.node_id.clone();
            if address_space.insert_node(node) {
                if self.config.auto_references {
                    address_space.add_reference(Reference::has_component(
                        parent_id.clone(),
                        node_id.clone(),
                    ));
                }
                node_ids.push(node_id);
                self.created_count.fetch_add(1, Ordering::Relaxed);
            } else {
                self.failed_count.fetch_add(1, Ordering::Relaxed);
            }

            // Report progress periodically
            if self.created_count.load(Ordering::Relaxed) % self.config.progress_interval == 0 {
                self.report_progress(total);
            }
        }

        self.report_progress(total);
        Ok(node_ids)
    }

    /// Create an entire subtree from an object template.
    pub fn create_subtree(
        &self,
        address_space: &AddressSpace,
        parent_id: &NodeId,
        template: &ObjectTemplate,
        namespace: u16,
        base_id: u32,
        instance_count: usize,
    ) -> OpcUaResult<Vec<NodeId>> {
        self.reset_counters();
        let nodes_per_instance = template.total_node_count();
        let total = nodes_per_instance * instance_count;
        let mut all_root_ids = Vec::with_capacity(instance_count);
        let mut current_id = base_id;

        for instance_idx in 0..instance_count {
            let (root_id, next_id) = self.create_subtree_recursive(
                address_space,
                parent_id,
                template,
                namespace,
                current_id,
                instance_idx,
                total,
            )?;
            all_root_ids.push(root_id);
            current_id = next_id;
        }

        Ok(all_root_ids)
    }

    fn create_subtree_recursive(
        &self,
        address_space: &AddressSpace,
        parent_id: &NodeId,
        template: &ObjectTemplate,
        namespace: u16,
        mut current_id: u32,
        instance_idx: usize,
        total: usize,
    ) -> OpcUaResult<(NodeId, u32)> {
        // Create the object node
        let object_node_id = NodeId::numeric(namespace, current_id);
        current_id += 1;

        let object_name = format!("{}_{}", template.name_prefix, instance_idx);
        let object = ObjectBuilder::new(object_node_id.clone())
            .browse_name(namespace, &object_name)
            .display_name(&object_name)
            .build();

        if address_space.insert_node(object) {
            address_space.add_reference(Reference::organizes(
                parent_id.clone(),
                object_node_id.clone(),
            ));
            self.created_count.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_count.fetch_add(1, Ordering::Relaxed);
        }

        // Create variable children
        for (var_idx, var_template) in template.variables.iter().enumerate() {
            let var_node_id = NodeId::numeric(namespace, current_id);
            current_id += 1;

            let var_name = format!("{}_{}", var_template.name_prefix, var_idx);
            let value = var_template.value_type.generate(var_idx);

            let mut builder = VariableBuilder::new(var_node_id.clone())
                .browse_name(namespace, &var_name)
                .display_name(&var_name)
                .data_type(var_template.data_type)
                .value(value)
                .sampling_interval(var_template.sampling_interval);

            if var_template.writable {
                builder = builder.writable();
            }
            if var_template.historizing {
                builder = builder.historizing();
            }

            let variable = builder.build();
            if address_space.insert_node(variable) {
                address_space.add_reference(Reference::has_component(
                    object_node_id.clone(),
                    var_node_id,
                ));
                self.created_count.fetch_add(1, Ordering::Relaxed);
            } else {
                self.failed_count.fetch_add(1, Ordering::Relaxed);
            }

            // Report progress
            if self.created_count.load(Ordering::Relaxed) % self.config.progress_interval == 0 {
                self.report_progress(total);
            }
        }

        // Create child object subtrees
        for (child_idx, child_template) in template.children.iter().enumerate() {
            let (_, next_id) = self.create_subtree_recursive(
                address_space,
                &object_node_id,
                child_template,
                namespace,
                current_id,
                child_idx,
                total,
            )?;
            current_id = next_id;
        }

        Ok((object_node_id, current_id))
    }

    /// Get current progress.
    pub fn get_progress(&self, total: usize) -> BatchProgress {
        let created = self.created_count.load(Ordering::Relaxed);
        let failed = self.failed_count.load(Ordering::Relaxed);
        let percent_complete = if total > 0 {
            ((created + failed) as f64 / total as f64) * 100.0
        } else {
            100.0
        };

        BatchProgress {
            total,
            created,
            failed,
            percent_complete,
        }
    }

    fn reset_counters(&self) {
        self.created_count.store(0, Ordering::Relaxed);
        self.failed_count.store(0, Ordering::Relaxed);
    }

    fn report_progress(&self, total: usize) {
        if let Some(ref callback) = self.progress_callback {
            callback(self.get_progress(total));
        }
    }
}

impl Default for BatchNodeCreator {
    fn default() -> Self {
        Self::new(BatchConfig::default())
    }
}

/// Industrial device template presets.
pub mod presets {
    use super::*;

    /// Create a standard HVAC device template.
    pub fn hvac_device() -> ObjectTemplate {
        ObjectTemplate::new("HVAC")
            .with_variable(
                VariableTemplate::new("SupplyTemp", DataTypeId::Double).with_value_type(
                    ValueGeneratorType::Random {
                        min: 18.0,
                        max: 28.0,
                    },
                ),
            )
            .with_variable(
                VariableTemplate::new("ReturnTemp", DataTypeId::Double).with_value_type(
                    ValueGeneratorType::Random {
                        min: 20.0,
                        max: 26.0,
                    },
                ),
            )
            .with_variable(
                VariableTemplate::new("SetPoint", DataTypeId::Double)
                    .with_value_type(ValueGeneratorType::Fixed(22.0))
                    .writable(),
            )
            .with_variable(
                VariableTemplate::new("FanSpeed", DataTypeId::Double).with_value_type(
                    ValueGeneratorType::Random {
                        min: 0.0,
                        max: 100.0,
                    },
                ),
            )
            .with_variable(
                VariableTemplate::new("DamperPosition", DataTypeId::Double)
                    .with_value_type(ValueGeneratorType::Random {
                        min: 0.0,
                        max: 100.0,
                    })
                    .writable(),
            )
            .with_variable(
                VariableTemplate::new("CoolingValve", DataTypeId::Double).with_value_type(
                    ValueGeneratorType::Random {
                        min: 0.0,
                        max: 100.0,
                    },
                ),
            )
            .with_variable(
                VariableTemplate::new("HeatingValve", DataTypeId::Double).with_value_type(
                    ValueGeneratorType::Random {
                        min: 0.0,
                        max: 100.0,
                    },
                ),
            )
            .with_variable(
                VariableTemplate::new("OccupancyStatus", DataTypeId::Boolean)
                    .with_value_type(ValueGeneratorType::Fixed(1.0)),
            )
    }

    /// Create a standard sensor array template.
    pub fn sensor_array(sensor_count: usize) -> ObjectTemplate {
        let mut template = ObjectTemplate::new("SensorArray");

        for i in 0..sensor_count {
            template = template.with_variable(
                VariableTemplate::new(format!("Sensor{}", i), DataTypeId::Double)
                    .with_value_type(ValueGeneratorType::SineWave {
                        amplitude: 100.0,
                        period: 100,
                    })
                    .historizing(),
            );
        }

        template
    }

    /// Create a PLC template with typical I/O points.
    pub fn plc_io(
        digital_inputs: usize,
        digital_outputs: usize,
        analog_inputs: usize,
        analog_outputs: usize,
    ) -> ObjectTemplate {
        let mut template = ObjectTemplate::new("PLC");

        // Digital inputs
        let mut di_template = ObjectTemplate::new("DigitalInputs");
        for i in 0..digital_inputs {
            di_template = di_template.with_variable(
                VariableTemplate::new(format!("DI{}", i), DataTypeId::Boolean)
                    .with_value_type(ValueGeneratorType::Zero),
            );
        }
        template = template.with_child(di_template);

        // Digital outputs
        let mut do_template = ObjectTemplate::new("DigitalOutputs");
        for i in 0..digital_outputs {
            do_template = do_template.with_variable(
                VariableTemplate::new(format!("DO{}", i), DataTypeId::Boolean)
                    .with_value_type(ValueGeneratorType::Zero)
                    .writable(),
            );
        }
        template = template.with_child(do_template);

        // Analog inputs
        let mut ai_template = ObjectTemplate::new("AnalogInputs");
        for i in 0..analog_inputs {
            ai_template = ai_template.with_variable(
                VariableTemplate::new(format!("AI{}", i), DataTypeId::Double)
                    .with_value_type(ValueGeneratorType::Random {
                        min: 0.0,
                        max: 100.0,
                    })
                    .historizing(),
            );
        }
        template = template.with_child(ai_template);

        // Analog outputs
        let mut ao_template = ObjectTemplate::new("AnalogOutputs");
        for i in 0..analog_outputs {
            ao_template = ao_template.with_variable(
                VariableTemplate::new(format!("AO{}", i), DataTypeId::Double)
                    .with_value_type(ValueGeneratorType::Zero)
                    .writable(),
            );
        }
        template = template.with_child(ao_template);

        template
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::AddressSpaceConfig;

    #[test]
    fn test_variable_template() {
        let template = VariableTemplate::new("Temperature", DataTypeId::Double)
            .with_value_type(ValueGeneratorType::Random {
                min: 0.0,
                max: 100.0,
            })
            .writable()
            .historizing()
            .sampling_interval(100.0);

        assert_eq!(template.name_prefix, "Temperature");
        assert!(template.writable);
        assert!(template.historizing);
    }

    #[test]
    fn test_value_generator() {
        let zero = ValueGeneratorType::Zero;
        assert_eq!(zero.generate(0), Variant::Double(0.0));

        let sequential = ValueGeneratorType::Sequential;
        assert_eq!(sequential.generate(5), Variant::Double(5.0));

        let fixed = ValueGeneratorType::Fixed(42.0);
        assert_eq!(fixed.generate(0), Variant::Double(42.0));
    }

    #[test]
    fn test_object_template_count() {
        let template = ObjectTemplate::new("Device")
            .with_variable(VariableTemplate::new("Var1", DataTypeId::Double))
            .with_variable(VariableTemplate::new("Var2", DataTypeId::Double))
            .with_child(
                ObjectTemplate::new("Child")
                    .with_variable(VariableTemplate::new("ChildVar", DataTypeId::Double)),
            );

        // Device(1) + 2 vars + Child(1) + 1 var = 5
        assert_eq!(template.total_node_count(), 5);
    }

    #[test]
    fn test_batch_create_variables() {
        let address_space = AddressSpace::new(AddressSpaceConfig::default());
        let creator = BatchNodeCreator::new(BatchConfig::small());

        let parent_id = NodeId::numeric(0, 85); // Objects folder
        let template = VariableTemplate::new("TestVar", DataTypeId::Double)
            .with_value_type(ValueGeneratorType::Sequential);

        let node_ids = creator
            .create_variables_from_template(&address_space, &parent_id, &template, 2, 1000, 100)
            .unwrap();

        assert_eq!(node_ids.len(), 100);

        // Verify nodes exist
        for node_id in &node_ids {
            assert!(address_space.get_node(node_id).is_some());
        }
    }

    #[test]
    fn test_batch_create_subtree() {
        let address_space = AddressSpace::new(AddressSpaceConfig::default());
        let creator = BatchNodeCreator::new(BatchConfig::small());

        let template = presets::hvac_device();
        let parent_id = NodeId::numeric(0, 85);

        let root_ids = creator
            .create_subtree(
                &address_space,
                &parent_id,
                &template,
                2,
                1000,
                5, // Create 5 HVAC devices
            )
            .unwrap();

        assert_eq!(root_ids.len(), 5);

        // Each HVAC device has 1 object + 8 variables = 9 nodes
        let progress = creator.get_progress(45);
        assert_eq!(progress.created, 45);
    }

    #[test]
    fn test_presets() {
        let hvac = presets::hvac_device();
        assert_eq!(hvac.name_prefix, "HVAC");
        assert_eq!(hvac.variables.len(), 8);

        let sensors = presets::sensor_array(10);
        assert_eq!(sensors.variables.len(), 10);

        let plc = presets::plc_io(8, 8, 4, 4);
        assert_eq!(plc.children.len(), 4);
    }

    #[test]
    fn test_progress_callback() {
        use std::sync::atomic::AtomicBool;

        let address_space = AddressSpace::new(AddressSpaceConfig::default());
        let callback_called = Arc::new(AtomicBool::new(false));
        let callback_called_clone = callback_called.clone();

        let creator = BatchNodeCreator::new(BatchConfig {
            progress_interval: 10,
            ..BatchConfig::small()
        })
        .with_progress_callback(Arc::new(move |progress| {
            callback_called_clone.store(true, Ordering::Relaxed);
            assert!(progress.percent_complete <= 100.0);
        }));

        let template = VariableTemplate::new("Test", DataTypeId::Double);
        let _ = creator.create_variables_from_template(
            &address_space,
            &NodeId::numeric(0, 85),
            &template,
            2,
            1000,
            50,
        );

        assert!(callback_called.load(Ordering::Relaxed));
    }
}
