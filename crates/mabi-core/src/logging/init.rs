//! Logging initialization and setup.
//!
//! This module handles the initialization of the tracing subscriber
//! with various configurations including file logging with rotation.

use std::path::Path;
use std::sync::OnceLock;

use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    fmt::{self, format::FmtSpan, time::UtcTime},
    layer::SubscriberExt,
    reload,
    util::SubscriberInitExt,
    EnvFilter,
};

use super::config::{LogConfig, LogFormat, LogTarget};
use super::dynamic::LogLevelController;
use super::rotation::RotationStrategy;
use crate::error::Result;

/// Global initialization guard to prevent double initialization.
static LOGGING_INITIALIZED: OnceLock<()> = OnceLock::new();

/// Initialize logging with the given configuration.
///
/// This function should be called once at application startup.
/// Subsequent calls will be ignored (returns Ok without error).
///
/// # Arguments
/// * `config` - The logging configuration
///
/// # Returns
/// * `Ok(())` - Logging was initialized successfully or was already initialized
/// * `Err(Error)` - Logging initialization failed
///
/// # Example
///
/// ```rust,no_run
/// use mabi_core::logging::{init_logging, LogConfig};
///
/// init_logging(&LogConfig::default()).expect("Failed to initialize logging");
/// ```
pub fn init_logging(config: &LogConfig) -> Result<()> {
    // Prevent double initialization
    if LOGGING_INITIALIZED.get().is_some() {
        return Ok(());
    }

    // Validate configuration
    config.validate()?;

    let result = init_logging_internal(config);

    if result.is_ok() {
        LOGGING_INITIALIZED.get_or_init(|| ());
    }

    result
}

/// Internal initialization logic.
fn init_logging_internal(config: &LogConfig) -> Result<()> {
    let filter = build_env_filter(config);
    let span_events = build_span_events(config);

    // Check if we need dynamic level support
    if config.dynamic_level {
        init_with_dynamic_level(config, filter, span_events)
    } else {
        init_static(config, filter, span_events)
    }
}

/// Initialize with dynamic log level support.
fn init_with_dynamic_level(
    config: &LogConfig,
    filter: EnvFilter,
    span_events: FmtSpan,
) -> Result<()> {
    let (filter_layer, reload_handle) = reload::Layer::new(filter);

    match &config.target {
        LogTarget::Stdout => {
            init_stdout_with_reload(config, filter_layer, span_events)?;
        }
        LogTarget::Stderr => {
            init_stderr_with_reload(config, filter_layer, span_events)?;
        }
        LogTarget::File {
            directory,
            filename_prefix,
            rotation,
        } => {
            init_file_with_reload(
                config,
                filter_layer,
                span_events,
                directory,
                filename_prefix,
                rotation,
            )?;
        }
        LogTarget::Multi(targets) => {
            // For multi-target, we'll initialize stdout as the primary with reload
            // and add file as a secondary layer
            init_multi_with_reload(config, filter_layer, span_events, targets)?;
        }
    }

    // Register the controller
    let controller = LogLevelController::new(reload_handle, config.level);
    controller.register_global();

    Ok(())
}

/// Initialize stdout with reload support.
fn init_stdout_with_reload(
    config: &LogConfig,
    filter_layer: reload::Layer<EnvFilter, tracing_subscriber::Registry>,
    span_events: FmtSpan,
) -> Result<()> {
    let registry = tracing_subscriber::registry().with(filter_layer);

    match config.format {
        LogFormat::Json => {
            let layer = fmt::layer()
                .json()
                .with_timer(UtcTime::rfc_3339())
                .with_file(config.include_location)
                .with_line_number(config.include_location)
                .with_target(config.include_target)
                .with_thread_ids(config.include_thread_ids)
                .with_thread_names(config.include_thread_names)
                .with_span_events(span_events)
                .with_writer(std::io::stdout);
            registry.with(layer).init();
        }
        LogFormat::Pretty => {
            let layer = fmt::layer()
                .pretty()
                .with_timer(UtcTime::rfc_3339())
                .with_file(config.include_location)
                .with_line_number(config.include_location)
                .with_target(config.include_target)
                .with_thread_ids(config.include_thread_ids)
                .with_thread_names(config.include_thread_names)
                .with_span_events(span_events)
                .with_ansi(config.ansi_colors)
                .with_writer(std::io::stdout);
            registry.with(layer).init();
        }
        LogFormat::Compact => {
            let layer = fmt::layer()
                .compact()
                .with_timer(UtcTime::rfc_3339())
                .with_file(config.include_location)
                .with_line_number(config.include_location)
                .with_target(config.include_target)
                .with_thread_ids(config.include_thread_ids)
                .with_thread_names(config.include_thread_names)
                .with_span_events(span_events)
                .with_ansi(config.ansi_colors)
                .with_writer(std::io::stdout);
            registry.with(layer).init();
        }
        LogFormat::Full => {
            let layer = fmt::layer()
                .with_timer(UtcTime::rfc_3339())
                .with_file(config.include_location)
                .with_line_number(config.include_location)
                .with_target(config.include_target)
                .with_thread_ids(config.include_thread_ids)
                .with_thread_names(config.include_thread_names)
                .with_span_events(span_events)
                .with_ansi(config.ansi_colors)
                .with_writer(std::io::stdout);
            registry.with(layer).init();
        }
    }

    Ok(())
}

/// Initialize stderr with reload support.
fn init_stderr_with_reload(
    config: &LogConfig,
    filter_layer: reload::Layer<EnvFilter, tracing_subscriber::Registry>,
    span_events: FmtSpan,
) -> Result<()> {
    let registry = tracing_subscriber::registry().with(filter_layer);

    let layer = fmt::layer()
        .with_timer(UtcTime::rfc_3339())
        .with_file(config.include_location)
        .with_line_number(config.include_location)
        .with_target(config.include_target)
        .with_thread_ids(config.include_thread_ids)
        .with_thread_names(config.include_thread_names)
        .with_span_events(span_events)
        .with_ansi(config.ansi_colors)
        .with_writer(std::io::stderr);

    registry.with(layer).init();
    Ok(())
}

/// Initialize file logging with rotation and reload support.
fn init_file_with_reload(
    config: &LogConfig,
    filter_layer: reload::Layer<EnvFilter, tracing_subscriber::Registry>,
    span_events: FmtSpan,
    directory: &Path,
    filename_prefix: &str,
    rotation_config: &super::rotation::RotationConfig,
) -> Result<()> {
    let rotation = match rotation_config.strategy {
        RotationStrategy::Daily => Rotation::DAILY,
        RotationStrategy::Hourly => Rotation::HOURLY,
        RotationStrategy::Minutely => Rotation::MINUTELY,
        RotationStrategy::Never => Rotation::NEVER,
    };

    let file_appender = RollingFileAppender::new(rotation, directory, filename_prefix);
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Keep the guard alive for the lifetime of the program
    // by leaking it (it will be cleaned up when the program exits)
    std::mem::forget(_guard);

    let registry = tracing_subscriber::registry().with(filter_layer);

    // File logging always uses JSON format for structured parsing
    let layer = fmt::layer()
        .json()
        .with_timer(UtcTime::rfc_3339())
        .with_file(config.include_location)
        .with_line_number(config.include_location)
        .with_target(config.include_target)
        .with_thread_ids(config.include_thread_ids)
        .with_thread_names(config.include_thread_names)
        .with_span_events(span_events)
        .with_ansi(false) // No ANSI codes in file output
        .with_writer(non_blocking);

    registry.with(layer).init();
    Ok(())
}

/// Initialize multi-target logging with reload support.
fn init_multi_with_reload(
    config: &LogConfig,
    filter_layer: reload::Layer<EnvFilter, tracing_subscriber::Registry>,
    span_events: FmtSpan,
    targets: &[LogTarget],
) -> Result<()> {
    // For simplicity, we handle stdout/stderr + file combination
    // More complex combinations could be added as needed

    let has_console = targets.iter().any(|t| matches!(t, LogTarget::Stdout | LogTarget::Stderr));
    let file_target = targets.iter().find_map(|t| {
        if let LogTarget::File { directory, filename_prefix, rotation } = t {
            Some((directory.clone(), filename_prefix.clone(), rotation.clone()))
        } else {
            None
        }
    });

    let registry = tracing_subscriber::registry().with(filter_layer);

    if has_console && file_target.is_some() {
        let (directory, filename_prefix, rotation_config) = file_target.unwrap();

        let rotation = match rotation_config.strategy {
            RotationStrategy::Daily => Rotation::DAILY,
            RotationStrategy::Hourly => Rotation::HOURLY,
            RotationStrategy::Minutely => Rotation::MINUTELY,
            RotationStrategy::Never => Rotation::NEVER,
        };

        let file_appender = RollingFileAppender::new(rotation, &directory, &filename_prefix);
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
        std::mem::forget(_guard);

        // Console layer (pretty format)
        let console_layer = fmt::layer()
            .pretty()
            .with_timer(UtcTime::rfc_3339())
            .with_file(config.include_location)
            .with_line_number(config.include_location)
            .with_target(config.include_target)
            .with_span_events(span_events.clone())
            .with_ansi(config.ansi_colors)
            .with_writer(std::io::stdout);

        // File layer (JSON format)
        let file_layer = fmt::layer()
            .json()
            .with_timer(UtcTime::rfc_3339())
            .with_file(config.include_location)
            .with_line_number(config.include_location)
            .with_target(config.include_target)
            .with_span_events(span_events)
            .with_ansi(false)
            .with_writer(non_blocking);

        registry.with(console_layer).with(file_layer).init();
    } else if has_console {
        // Just console
        init_stdout_with_reload(config, reload::Layer::new(build_env_filter(config)).0, span_events)?;
    } else if let Some((directory, filename_prefix, rotation_config)) = file_target {
        // Just file
        let reload_layer = reload::Layer::new(build_env_filter(config)).0;
        init_file_with_reload(config, reload_layer, span_events, &directory, &filename_prefix, &rotation_config)?;
    }

    Ok(())
}

/// Initialize without dynamic level support (simpler, slightly more efficient).
fn init_static(config: &LogConfig, filter: EnvFilter, span_events: FmtSpan) -> Result<()> {
    match &config.target {
        LogTarget::Stdout => init_stdout_static(config, filter, span_events),
        LogTarget::Stderr => init_stderr_static(config, filter, span_events),
        LogTarget::File {
            directory,
            filename_prefix,
            rotation,
        } => init_file_static(config, filter, span_events, directory, filename_prefix, rotation),
        LogTarget::Multi(_) => {
            // For multi-target without dynamic, we use the same logic as with dynamic
            // but don't register a controller
            init_with_dynamic_level(config, filter, span_events)
        }
    }
}

/// Initialize stdout without reload support.
fn init_stdout_static(config: &LogConfig, filter: EnvFilter, span_events: FmtSpan) -> Result<()> {
    let registry = tracing_subscriber::registry().with(filter);

    match config.format {
        LogFormat::Json => {
            let layer = fmt::layer()
                .json()
                .with_timer(UtcTime::rfc_3339())
                .with_file(config.include_location)
                .with_line_number(config.include_location)
                .with_target(config.include_target)
                .with_thread_ids(config.include_thread_ids)
                .with_thread_names(config.include_thread_names)
                .with_span_events(span_events)
                .with_writer(std::io::stdout);
            registry.with(layer).init();
        }
        LogFormat::Pretty => {
            let layer = fmt::layer()
                .pretty()
                .with_timer(UtcTime::rfc_3339())
                .with_file(config.include_location)
                .with_line_number(config.include_location)
                .with_target(config.include_target)
                .with_thread_ids(config.include_thread_ids)
                .with_thread_names(config.include_thread_names)
                .with_span_events(span_events)
                .with_ansi(config.ansi_colors)
                .with_writer(std::io::stdout);
            registry.with(layer).init();
        }
        LogFormat::Compact => {
            let layer = fmt::layer()
                .compact()
                .with_timer(UtcTime::rfc_3339())
                .with_file(config.include_location)
                .with_line_number(config.include_location)
                .with_target(config.include_target)
                .with_thread_ids(config.include_thread_ids)
                .with_thread_names(config.include_thread_names)
                .with_span_events(span_events)
                .with_ansi(config.ansi_colors)
                .with_writer(std::io::stdout);
            registry.with(layer).init();
        }
        LogFormat::Full => {
            let layer = fmt::layer()
                .with_timer(UtcTime::rfc_3339())
                .with_file(config.include_location)
                .with_line_number(config.include_location)
                .with_target(config.include_target)
                .with_thread_ids(config.include_thread_ids)
                .with_thread_names(config.include_thread_names)
                .with_span_events(span_events)
                .with_ansi(config.ansi_colors)
                .with_writer(std::io::stdout);
            registry.with(layer).init();
        }
    }

    Ok(())
}

/// Initialize stderr without reload support.
fn init_stderr_static(config: &LogConfig, filter: EnvFilter, span_events: FmtSpan) -> Result<()> {
    let registry = tracing_subscriber::registry().with(filter);

    let layer = fmt::layer()
        .with_timer(UtcTime::rfc_3339())
        .with_file(config.include_location)
        .with_line_number(config.include_location)
        .with_target(config.include_target)
        .with_thread_ids(config.include_thread_ids)
        .with_thread_names(config.include_thread_names)
        .with_span_events(span_events)
        .with_ansi(config.ansi_colors)
        .with_writer(std::io::stderr);

    registry.with(layer).init();
    Ok(())
}

/// Initialize file logging without reload support.
fn init_file_static(
    config: &LogConfig,
    filter: EnvFilter,
    span_events: FmtSpan,
    directory: &Path,
    filename_prefix: &str,
    rotation_config: &super::rotation::RotationConfig,
) -> Result<()> {
    let rotation = match rotation_config.strategy {
        RotationStrategy::Daily => Rotation::DAILY,
        RotationStrategy::Hourly => Rotation::HOURLY,
        RotationStrategy::Minutely => Rotation::MINUTELY,
        RotationStrategy::Never => Rotation::NEVER,
    };

    let file_appender = RollingFileAppender::new(rotation, directory, filename_prefix);
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    std::mem::forget(_guard);

    let registry = tracing_subscriber::registry().with(filter);

    let layer = fmt::layer()
        .json()
        .with_timer(UtcTime::rfc_3339())
        .with_file(config.include_location)
        .with_line_number(config.include_location)
        .with_target(config.include_target)
        .with_thread_ids(config.include_thread_ids)
        .with_thread_names(config.include_thread_names)
        .with_span_events(span_events)
        .with_ansi(false)
        .with_writer(non_blocking);

    registry.with(layer).init();
    Ok(())
}

/// Build an EnvFilter from the configuration.
fn build_env_filter(config: &LogConfig) -> EnvFilter {
    // First, try to get filter from environment variable
    EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let filter_str = config.build_filter_string();
        EnvFilter::try_new(&filter_str).unwrap_or_else(|_| {
            EnvFilter::new(config.level.as_filter_str())
        })
    })
}

/// Build FmtSpan from configuration.
fn build_span_events(config: &LogConfig) -> FmtSpan {
    if config.include_span_events {
        FmtSpan::ENTER | FmtSpan::EXIT
    } else {
        FmtSpan::NONE
    }
}

/// Initialize logging for tests.
///
/// This uses a test-friendly configuration and ignores errors
/// from multiple initializations (common in tests).
pub fn init_test_logging() {
    let _ = init_logging(&LogConfig::test());
}

/// Check if logging has been initialized.
pub fn is_logging_initialized() -> bool {
    LOGGING_INITIALIZED.get().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::config::LogLevel;

    #[test]
    fn test_build_env_filter() {
        let config = LogConfig::builder().level(LogLevel::Debug).build();
        let filter = build_env_filter(&config);
        // Just verify it doesn't panic
        drop(filter);
    }

    #[test]
    fn test_build_span_events() {
        let config = LogConfig::builder().include_span_events(true).build();
        let span_events = build_span_events(&config);
        // FmtSpan doesn't have is_empty, check it's configured correctly
        assert_ne!(span_events, FmtSpan::NONE);

        let config = LogConfig::builder().include_span_events(false).build();
        let span_events = build_span_events(&config);
        assert_eq!(span_events, FmtSpan::NONE);
    }
}
