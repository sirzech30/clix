use tracing_subscriber::EnvFilter;

pub fn init_logging(debug: bool) {
    let default_level = if debug { "clix=debug" } else { "clix=info" };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
