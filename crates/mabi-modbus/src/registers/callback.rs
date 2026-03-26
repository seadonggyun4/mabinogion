//! Callback system for register store observation.
//!
//! This module provides a flexible callback system that allows external components
//! (like the scenario engine or metrics collector) to observe register read/write operations.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::RwLock;

use super::{RegisterType, RegisterValue};

/// Priority for callback execution.
///
/// Higher priority callbacks are executed first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CallbackPriority {
    /// Low priority - executed last (e.g., logging).
    Low = 0,
    /// Normal priority - default.
    Normal = 50,
    /// High priority - executed first (e.g., validation).
    High = 100,
}

impl Default for CallbackPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Read callback context.
#[derive(Debug, Clone, Copy)]
pub struct ReadContext {
    /// Register type being read.
    pub register_type: RegisterType,
    /// Start address of the read operation.
    pub address: u16,
    /// Number of registers being read.
    pub count: u16,
}

/// Write callback context.
#[derive(Debug, Clone, Copy)]
pub struct WriteContext {
    /// Register type being written.
    pub register_type: RegisterType,
    /// Address of the write operation.
    pub address: u16,
    /// Old value before the write.
    pub old_value: RegisterValue,
    /// New value after the write.
    pub new_value: RegisterValue,
}

impl WriteContext {
    /// Check if the value actually changed.
    #[inline]
    pub fn has_changed(&self) -> bool {
        self.old_value != self.new_value
    }
}

/// Trait for read callbacks.
///
/// Implement this trait to observe read operations on the register store.
pub trait ReadCallback: Send + Sync {
    /// Called when registers are read.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Context information about the read operation.
    /// * `values` - The values that were read.
    fn on_read(&self, ctx: ReadContext, values: &[RegisterValue]);

    /// Get the priority of this callback.
    fn priority(&self) -> CallbackPriority {
        CallbackPriority::Normal
    }

    /// Get a unique identifier for this callback (for removal).
    fn id(&self) -> &str {
        "unnamed"
    }
}

/// Trait for write callbacks.
///
/// Implement this trait to observe write operations on the register store.
pub trait WriteCallback: Send + Sync {
    /// Called when a register is written.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Context information about the write operation.
    fn on_write(&self, ctx: WriteContext);

    /// Get the priority of this callback.
    fn priority(&self) -> CallbackPriority {
        CallbackPriority::Normal
    }

    /// Get a unique identifier for this callback (for removal).
    fn id(&self) -> &str {
        "unnamed"
    }
}

/// Function-based read callback wrapper.
pub struct ReadCallbackFn<F>
where
    F: Fn(ReadContext, &[RegisterValue]) + Send + Sync,
{
    id: String,
    priority: CallbackPriority,
    func: F,
}

impl<F> ReadCallbackFn<F>
where
    F: Fn(ReadContext, &[RegisterValue]) + Send + Sync,
{
    /// Create a new function-based read callback.
    pub fn new(id: impl Into<String>, func: F) -> Self {
        Self {
            id: id.into(),
            priority: CallbackPriority::Normal,
            func,
        }
    }

    /// Set the priority.
    pub fn with_priority(mut self, priority: CallbackPriority) -> Self {
        self.priority = priority;
        self
    }
}

impl<F> ReadCallback for ReadCallbackFn<F>
where
    F: Fn(ReadContext, &[RegisterValue]) + Send + Sync,
{
    fn on_read(&self, ctx: ReadContext, values: &[RegisterValue]) {
        (self.func)(ctx, values);
    }

    fn priority(&self) -> CallbackPriority {
        self.priority
    }

    fn id(&self) -> &str {
        &self.id
    }
}

/// Function-based write callback wrapper.
pub struct WriteCallbackFn<F>
where
    F: Fn(WriteContext) + Send + Sync,
{
    id: String,
    priority: CallbackPriority,
    func: F,
}

impl<F> WriteCallbackFn<F>
where
    F: Fn(WriteContext) + Send + Sync,
{
    /// Create a new function-based write callback.
    pub fn new(id: impl Into<String>, func: F) -> Self {
        Self {
            id: id.into(),
            priority: CallbackPriority::Normal,
            func,
        }
    }

    /// Set the priority.
    pub fn with_priority(mut self, priority: CallbackPriority) -> Self {
        self.priority = priority;
        self
    }
}

impl<F> WriteCallback for WriteCallbackFn<F>
where
    F: Fn(WriteContext) + Send + Sync,
{
    fn on_write(&self, ctx: WriteContext) {
        (self.func)(ctx);
    }

    fn priority(&self) -> CallbackPriority {
        self.priority
    }

    fn id(&self) -> &str {
        &self.id
    }
}

/// Internal entry for storing read callbacks with metadata.
struct ReadCallbackEntry {
    callback: Arc<dyn ReadCallback>,
    priority: CallbackPriority,
    id: String,
}

/// Internal entry for storing write callbacks with metadata.
struct WriteCallbackEntry {
    callback: Arc<dyn WriteCallback>,
    priority: CallbackPriority,
    id: String,
}

/// Manager for read and write callbacks.
///
/// This struct handles registration, removal, and invocation of callbacks
/// in a thread-safe manner.
pub struct CallbackManager {
    /// Read callbacks sorted by priority (high to low).
    read_callbacks: RwLock<Vec<ReadCallbackEntry>>,
    /// Write callbacks sorted by priority (high to low).
    write_callbacks: RwLock<Vec<WriteCallbackEntry>>,
    /// Cached read callback count for hot-path checks.
    read_callback_count: AtomicUsize,
    /// Cached write callback count for hot-path checks.
    write_callback_count: AtomicUsize,
    /// Whether callbacks are enabled.
    enabled: std::sync::atomic::AtomicBool,
}

impl CallbackManager {
    /// Create a new callback manager.
    pub fn new() -> Self {
        Self {
            read_callbacks: RwLock::new(Vec::new()),
            write_callbacks: RwLock::new(Vec::new()),
            read_callback_count: AtomicUsize::new(0),
            write_callback_count: AtomicUsize::new(0),
            enabled: std::sync::atomic::AtomicBool::new(true),
        }
    }

    /// Check if callbacks are enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Enable or disable callbacks.
    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    /// Add a read callback.
    pub fn add_read_callback(&self, callback: Arc<dyn ReadCallback>) {
        let entry = ReadCallbackEntry {
            priority: callback.priority(),
            id: callback.id().to_string(),
            callback,
        };

        let mut callbacks = self.read_callbacks.write();
        callbacks.push(entry);
        // Sort by priority descending (high priority first)
        callbacks.sort_by(|a, b| b.priority.cmp(&a.priority));
        self.read_callback_count
            .store(callbacks.len(), Ordering::Relaxed);
    }

    /// Add a write callback.
    pub fn add_write_callback(&self, callback: Arc<dyn WriteCallback>) {
        let entry = WriteCallbackEntry {
            priority: callback.priority(),
            id: callback.id().to_string(),
            callback,
        };

        let mut callbacks = self.write_callbacks.write();
        callbacks.push(entry);
        callbacks.sort_by(|a, b| b.priority.cmp(&a.priority));
        self.write_callback_count
            .store(callbacks.len(), Ordering::Relaxed);
    }

    /// Remove a read callback by ID.
    pub fn remove_read_callback(&self, id: &str) -> bool {
        let mut callbacks = self.read_callbacks.write();
        let len_before = callbacks.len();
        callbacks.retain(|e| e.id != id);
        self.read_callback_count
            .store(callbacks.len(), Ordering::Relaxed);
        callbacks.len() < len_before
    }

    /// Remove a write callback by ID.
    pub fn remove_write_callback(&self, id: &str) -> bool {
        let mut callbacks = self.write_callbacks.write();
        let len_before = callbacks.len();
        callbacks.retain(|e| e.id != id);
        self.write_callback_count
            .store(callbacks.len(), Ordering::Relaxed);
        callbacks.len() < len_before
    }

    /// Clear all callbacks.
    pub fn clear(&self) {
        self.read_callbacks.write().clear();
        self.write_callbacks.write().clear();
        self.read_callback_count.store(0, Ordering::Relaxed);
        self.write_callback_count.store(0, Ordering::Relaxed);
    }

    /// Check whether any read callbacks are currently active.
    #[inline]
    pub fn has_read_callbacks(&self) -> bool {
        self.is_enabled() && self.read_callback_count.load(Ordering::Relaxed) > 0
    }

    /// Check whether any write callbacks are currently active.
    #[inline]
    pub fn has_write_callbacks(&self) -> bool {
        self.is_enabled() && self.write_callback_count.load(Ordering::Relaxed) > 0
    }

    /// Notify read callbacks.
    ///
    /// This is called by the register store after a read operation.
    #[inline]
    pub fn notify_read(&self, ctx: ReadContext, values: &[RegisterValue]) {
        if !self.has_read_callbacks() {
            return;
        }

        let callbacks = self.read_callbacks.read();
        for entry in callbacks.iter() {
            entry.callback.on_read(ctx, values);
        }
    }

    /// Notify write callbacks.
    ///
    /// This is called by the register store after a write operation.
    #[inline]
    pub fn notify_write(&self, ctx: WriteContext) {
        if !self.has_write_callbacks() {
            return;
        }

        let callbacks = self.write_callbacks.read();
        for entry in callbacks.iter() {
            entry.callback.on_write(ctx);
        }
    }

    /// Get the number of registered read callbacks.
    pub fn read_callback_count(&self) -> usize {
        self.read_callback_count.load(Ordering::Relaxed)
    }

    /// Get the number of registered write callbacks.
    pub fn write_callback_count(&self) -> usize {
        self.write_callback_count.load(Ordering::Relaxed)
    }
}

impl Default for CallbackManager {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CallbackManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackManager")
            .field("enabled", &self.is_enabled())
            .field("read_callbacks", &self.read_callback_count())
            .field("write_callbacks", &self.write_callback_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_callback_priority_ordering() {
        assert!(CallbackPriority::High > CallbackPriority::Normal);
        assert!(CallbackPriority::Normal > CallbackPriority::Low);
    }

    #[test]
    fn test_write_callback_fn() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let callback = WriteCallbackFn::new("test", move |_ctx| {
            counter_clone.fetch_add(1, Ordering::Relaxed);
        });

        let ctx = WriteContext {
            register_type: RegisterType::HoldingRegister,
            address: 0,
            old_value: RegisterValue::Word(0),
            new_value: RegisterValue::Word(100),
        };

        callback.on_write(ctx);
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_callback_manager() {
        let manager = CallbackManager::new();

        let read_counter = Arc::new(AtomicUsize::new(0));
        let write_counter = Arc::new(AtomicUsize::new(0));

        // Add read callback
        let read_counter_clone = read_counter.clone();
        manager.add_read_callback(Arc::new(ReadCallbackFn::new(
            "read_test",
            move |_ctx, _| {
                read_counter_clone.fetch_add(1, Ordering::Relaxed);
            },
        )));

        // Add write callback
        let write_counter_clone = write_counter.clone();
        manager.add_write_callback(Arc::new(WriteCallbackFn::new("write_test", move |_ctx| {
            write_counter_clone.fetch_add(1, Ordering::Relaxed);
        })));

        assert_eq!(manager.read_callback_count(), 1);
        assert_eq!(manager.write_callback_count(), 1);

        // Trigger callbacks
        let read_ctx = ReadContext {
            register_type: RegisterType::HoldingRegister,
            address: 0,
            count: 1,
        };
        manager.notify_read(read_ctx, &[RegisterValue::Word(100)]);
        assert_eq!(read_counter.load(Ordering::Relaxed), 1);

        let write_ctx = WriteContext {
            register_type: RegisterType::HoldingRegister,
            address: 0,
            old_value: RegisterValue::Word(0),
            new_value: RegisterValue::Word(100),
        };
        manager.notify_write(write_ctx);
        assert_eq!(write_counter.load(Ordering::Relaxed), 1);

        // Disable and verify
        manager.set_enabled(false);
        manager.notify_write(write_ctx);
        assert_eq!(write_counter.load(Ordering::Relaxed), 1); // Still 1

        // Re-enable
        manager.set_enabled(true);
        manager.notify_write(write_ctx);
        assert_eq!(write_counter.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_callback_removal() {
        let manager = CallbackManager::new();

        manager.add_write_callback(Arc::new(WriteCallbackFn::new("cb1", |_| {})));
        manager.add_write_callback(Arc::new(WriteCallbackFn::new("cb2", |_| {})));

        assert_eq!(manager.write_callback_count(), 2);

        assert!(manager.remove_write_callback("cb1"));
        assert_eq!(manager.write_callback_count(), 1);

        assert!(!manager.remove_write_callback("cb1")); // Already removed
        assert_eq!(manager.write_callback_count(), 1);

        manager.clear();
        assert_eq!(manager.write_callback_count(), 0);
    }

    #[test]
    fn test_callback_priority_ordering_in_manager() {
        let manager = CallbackManager::new();
        let order = Arc::new(RwLock::new(Vec::new()));

        // Add callbacks in random order
        let order1 = order.clone();
        manager.add_write_callback(Arc::new(
            WriteCallbackFn::new("normal", move |_| {
                order1.write().push("normal");
            })
            .with_priority(CallbackPriority::Normal),
        ));

        let order2 = order.clone();
        manager.add_write_callback(Arc::new(
            WriteCallbackFn::new("low", move |_| {
                order2.write().push("low");
            })
            .with_priority(CallbackPriority::Low),
        ));

        let order3 = order.clone();
        manager.add_write_callback(Arc::new(
            WriteCallbackFn::new("high", move |_| {
                order3.write().push("high");
            })
            .with_priority(CallbackPriority::High),
        ));

        let ctx = WriteContext {
            register_type: RegisterType::HoldingRegister,
            address: 0,
            old_value: RegisterValue::Word(0),
            new_value: RegisterValue::Word(1),
        };

        manager.notify_write(ctx);

        let execution_order = order.read().clone();
        assert_eq!(execution_order, vec!["high", "normal", "low"]);
    }
}
