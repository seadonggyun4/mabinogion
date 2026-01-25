//! Integration test framework core.
//!
//! Provides the foundational abstractions for integration testing.

use async_trait::async_trait;
use std::collections::HashMap;
use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio::time::timeout;

// =============================================================================
// Test Result Types
// =============================================================================

/// Test result indicating success or failure.
#[derive(Debug, Clone)]
pub enum TestResult {
    /// Test passed successfully.
    Passed,
    /// Test failed with message.
    Failed(String),
    /// Test was skipped with reason.
    Skipped(String),
    /// Test timed out.
    Timeout(Duration),
}

impl TestResult {
    pub fn is_success(&self) -> bool {
        matches!(self, TestResult::Passed)
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, TestResult::Failed(_) | TestResult::Timeout(_))
    }
}

/// Detailed test report.
#[derive(Debug, Clone)]
pub struct TestReport {
    pub name: String,
    pub result: TestResult,
    pub duration: Duration,
    pub assertions: Vec<AssertionResult>,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AssertionResult {
    pub name: String,
    pub passed: bool,
    pub message: Option<String>,
}

// =============================================================================
// Test Context
// =============================================================================

/// Test execution context with shared state.
pub struct TestContext {
    /// Test name.
    pub name: String,
    /// Test configuration.
    pub config: TestConfig,
    /// Shared state between test steps.
    state: Arc<RwLock<HashMap<String, Box<dyn std::any::Any + Send + Sync>>>>,
    /// Allocated ports.
    ports: Arc<Mutex<Vec<u16>>>,
    /// Test logs.
    logs: Arc<Mutex<Vec<String>>>,
    /// Assertions.
    assertions: Arc<Mutex<Vec<AssertionResult>>>,
}

impl TestContext {
    /// Create a new test context.
    pub fn new(name: impl Into<String>, config: TestConfig) -> Self {
        Self {
            name: name.into(),
            config,
            state: Arc::new(RwLock::new(HashMap::new())),
            ports: Arc::new(Mutex::new(Vec::new())),
            logs: Arc::new(Mutex::new(Vec::new())),
            assertions: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Store a value in the context state.
    pub async fn set<T: Send + Sync + 'static>(&self, key: impl Into<String>, value: T) {
        let mut state = self.state.write().await;
        state.insert(key.into(), Box::new(value));
    }

    /// Get a value from the context state.
    pub async fn get<T: Clone + Send + Sync + 'static>(&self, key: &str) -> Option<T> {
        let state = self.state.read().await;
        state.get(key).and_then(|v| v.downcast_ref::<T>().cloned())
    }

    /// Allocate a free port for testing.
    pub async fn allocate_port(&self) -> std::io::Result<u16> {
        let port = get_free_port().await?;
        self.ports.lock().await.push(port);
        Ok(port)
    }

    /// Log a message.
    pub async fn log(&self, msg: impl Into<String>) {
        let msg = msg.into();
        if self.config.verbose {
            println!("[{}] {}", self.name, msg);
        }
        self.logs.lock().await.push(msg);
    }

    /// Record an assertion.
    pub async fn assert(&self, name: impl Into<String>, passed: bool, message: Option<String>) {
        self.assertions.lock().await.push(AssertionResult {
            name: name.into(),
            passed,
            message,
        });
    }

    /// Assert equality.
    pub async fn assert_eq<T: PartialEq + Debug>(&self, name: &str, expected: T, actual: T) {
        let passed = expected == actual;
        let message = if passed {
            None
        } else {
            Some(format!("Expected {:?}, got {:?}", expected, actual))
        };
        self.assert(name, passed, message).await;
    }

    /// Assert that a condition is true.
    pub async fn assert_true(&self, name: &str, condition: bool) {
        let message = if condition {
            None
        } else {
            Some("Condition was false".into())
        };
        self.assert(name, condition, message).await;
    }

    /// Get all assertions.
    pub async fn get_assertions(&self) -> Vec<AssertionResult> {
        self.assertions.lock().await.clone()
    }

    /// Get all logs.
    pub async fn get_logs(&self) -> Vec<String> {
        self.logs.lock().await.clone()
    }

    /// Check if all assertions passed.
    pub async fn all_passed(&self) -> bool {
        self.assertions.lock().await.iter().all(|a| a.passed)
    }
}

/// Test configuration.
#[derive(Debug, Clone)]
pub struct TestConfig {
    /// Test timeout.
    pub timeout: Duration,
    /// Retry count on failure.
    pub retries: u32,
    /// Verbose output.
    pub verbose: bool,
    /// Parallel execution enabled.
    pub parallel: bool,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            retries: 0,
            verbose: false,
            parallel: true,
        }
    }
}

impl TestConfig {
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_retries(mut self, retries: u32) -> Self {
        self.retries = retries;
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }
}

// =============================================================================
// Test Case Trait
// =============================================================================

/// Trait for test cases.
#[async_trait]
pub trait TestCase: Send + Sync {
    /// Get the test name.
    fn name(&self) -> &str;

    /// Get the test description.
    fn description(&self) -> &str {
        ""
    }

    /// Check if this test should be skipped.
    fn should_skip(&self) -> Option<String> {
        None
    }

    /// Set up the test environment.
    async fn setup(&self, _ctx: &TestContext) -> Result<(), String> {
        Ok(())
    }

    /// Run the test.
    async fn run(&self, ctx: &TestContext) -> Result<(), String>;

    /// Clean up after the test.
    async fn teardown(&self, _ctx: &TestContext) -> Result<(), String> {
        Ok(())
    }
}

// =============================================================================
// Test Runner
// =============================================================================

/// Test runner for executing test suites.
pub struct TestRunner {
    tests: Vec<Box<dyn TestCase>>,
    config: TestConfig,
    hooks: Vec<Box<dyn TestHook>>,
}

impl TestRunner {
    /// Create a new test runner.
    pub fn new(config: TestConfig) -> Self {
        Self {
            tests: Vec::new(),
            config,
            hooks: Vec::new(),
        }
    }

    /// Add a test case.
    pub fn add_test(&mut self, test: impl TestCase + 'static) {
        self.tests.push(Box::new(test));
    }

    /// Add a test hook.
    pub fn add_hook(&mut self, hook: impl TestHook + 'static) {
        self.hooks.push(Box::new(hook));
    }

    /// Run all tests.
    pub async fn run(&self) -> TestSuiteReport {
        let mut reports = Vec::new();
        let start = std::time::Instant::now();

        for test in &self.tests {
            let report = self.run_test(test.as_ref()).await;
            reports.push(report);
        }

        let total_duration = start.elapsed();
        let passed = reports.iter().filter(|r| r.result.is_success()).count();
        let failed = reports.iter().filter(|r| r.result.is_failure()).count();
        let skipped = reports
            .iter()
            .filter(|r| matches!(r.result, TestResult::Skipped(_)))
            .count();

        TestSuiteReport {
            reports,
            total_duration,
            passed,
            failed,
            skipped,
        }
    }

    /// Run a single test with retries.
    async fn run_test(&self, test: &dyn TestCase) -> TestReport {
        // Check if should skip
        if let Some(reason) = test.should_skip() {
            return TestReport {
                name: test.name().to_string(),
                result: TestResult::Skipped(reason),
                duration: Duration::ZERO,
                assertions: Vec::new(),
                logs: Vec::new(),
            };
        }

        let mut last_result = None;
        let mut attempts = 0;

        while attempts <= self.config.retries {
            let ctx = TestContext::new(test.name(), self.config.clone());
            let start = std::time::Instant::now();

            // Run hooks before
            for hook in &self.hooks {
                if let Err(e) = hook.before_test(test.name()).await {
                    ctx.log(format!("Hook error: {}", e)).await;
                }
            }

            // Setup
            if let Err(e) = test.setup(&ctx).await {
                last_result = Some(TestReport {
                    name: test.name().to_string(),
                    result: TestResult::Failed(format!("Setup failed: {}", e)),
                    duration: start.elapsed(),
                    assertions: ctx.get_assertions().await,
                    logs: ctx.get_logs().await,
                });
                attempts += 1;
                continue;
            }

            // Run with timeout
            let run_result =
                timeout(self.config.timeout, test.run(&ctx)).await;

            let result = match run_result {
                Ok(Ok(())) => {
                    if ctx.all_passed().await {
                        TestResult::Passed
                    } else {
                        TestResult::Failed("Assertions failed".into())
                    }
                }
                Ok(Err(e)) => TestResult::Failed(e),
                Err(_) => TestResult::Timeout(self.config.timeout),
            };

            // Teardown
            if let Err(e) = test.teardown(&ctx).await {
                ctx.log(format!("Teardown error: {}", e)).await;
            }

            // Run hooks after
            for hook in &self.hooks {
                if let Err(e) = hook.after_test(test.name(), &result).await {
                    ctx.log(format!("Hook error: {}", e)).await;
                }
            }

            let report = TestReport {
                name: test.name().to_string(),
                result: result.clone(),
                duration: start.elapsed(),
                assertions: ctx.get_assertions().await,
                logs: ctx.get_logs().await,
            };

            if result.is_success() {
                return report;
            }

            last_result = Some(report);
            attempts += 1;
        }

        last_result.unwrap()
    }
}

/// Test hook for lifecycle events.
#[async_trait]
pub trait TestHook: Send + Sync {
    async fn before_test(&self, _name: &str) -> Result<(), String> {
        Ok(())
    }

    async fn after_test(&self, _name: &str, _result: &TestResult) -> Result<(), String> {
        Ok(())
    }
}

/// Test suite report.
#[derive(Debug)]
pub struct TestSuiteReport {
    pub reports: Vec<TestReport>,
    pub total_duration: Duration,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl TestSuiteReport {
    /// Print the report to stdout.
    pub fn print(&self) {
        println!("\n{}", "=".repeat(60));
        println!("TEST RESULTS");
        println!("{}", "=".repeat(60));

        for report in &self.reports {
            let status = match &report.result {
                TestResult::Passed => "\x1b[32m✓ PASSED\x1b[0m",
                TestResult::Failed(msg) => &format!("\x1b[31m✗ FAILED: {}\x1b[0m", msg),
                TestResult::Skipped(reason) => &format!("\x1b[33m○ SKIPPED: {}\x1b[0m", reason),
                TestResult::Timeout(d) => &format!("\x1b[31m⏱ TIMEOUT: {:?}\x1b[0m", d),
            };

            println!(
                "{} {} ({:?})",
                report.name,
                status,
                report.duration
            );

            // Print failed assertions
            for assertion in &report.assertions {
                if !assertion.passed {
                    println!(
                        "  └─ Assertion '{}' failed: {}",
                        assertion.name,
                        assertion.message.as_deref().unwrap_or("No message")
                    );
                }
            }
        }

        println!("{}", "-".repeat(60));
        println!(
            "Total: {} | Passed: {} | Failed: {} | Skipped: {} | Duration: {:?}",
            self.reports.len(),
            self.passed,
            self.failed,
            self.skipped,
            self.total_duration
        );
        println!("{}", "=".repeat(60));
    }

    /// Check if all tests passed.
    pub fn all_passed(&self) -> bool {
        self.failed == 0
    }
}

// =============================================================================
// Utilities
// =============================================================================

/// Get a free port for testing.
pub async fn get_free_port() -> std::io::Result<u16> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    Ok(addr.port())
}

/// Wait for a port to become available.
pub async fn wait_for_port(addr: SocketAddr, timeout_duration: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout_duration {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// Wait for a port to close.
pub async fn wait_for_port_closed(addr: SocketAddr, timeout_duration: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout_duration {
        if tokio::net::TcpStream::connect(addr).await.is_err() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}
