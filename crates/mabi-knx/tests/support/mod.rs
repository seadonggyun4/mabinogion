#![allow(dead_code)]

pub mod assertions;
pub mod contract;
pub mod fixtures;
pub mod frame_client;
pub mod interop;
pub mod runtime_harness;
pub mod server_harness;

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
