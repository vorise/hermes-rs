//! Logging configuration for Hermes Agent.
//!
//! Provides tracing subscriber setup with configurable log levels.

use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize the logging system.
///
/// Uses environment variable HERMES_LOG_LEVEL to set log level.
/// Default level is "info".
pub fn init_logging() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(
            std::env::var("HERMES_LOG_LEVEL")
                .unwrap_or_else(|_| "info".to_string())
        ));

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();
}

/// Initialize logging with a specific level.
pub fn init_logging_with_level(level: &str) {
    let filter = EnvFilter::new(level);

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();
}

/// Initialize logging for tests (quiet mode).
pub fn init_logging_quiet() {
    let filter = EnvFilter::new("warn");

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(false))
        .with(filter)
        .try_init()
        .ok(); // Ignore if already initialized
}