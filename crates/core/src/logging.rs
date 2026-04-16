/// Initialize the tracing subscriber for Hermes.
///
/// Respects `HERMES_DEBUG`, `HERMES_VERBOSE`, and `HERMES_QUIET` environment variables.
pub fn init_logging() {
    let env_filter = if std::env::var("HERMES_DEBUG").is_ok() {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug"))
    } else if std::env::var("HERMES_VERBOSE").is_ok() {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    } else if std::env::var("HERMES_QUIET").is_ok() {
        tracing_subscriber::EnvFilter::new("warn")
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .without_time()
        .try_init();
}
