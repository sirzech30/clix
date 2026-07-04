pub fn init_logging(debug: bool) {
    let filter = if debug {
        "clix=debug"
    } else {
        "clix=info"
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
