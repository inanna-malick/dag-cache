#[cfg(test)]
// spawn global tracing subscriber if not already initialized
pub fn init_test_env() {
    // initialize and register event/span logging subscriber
    let subscriber = tracing_subscriber::fmt::Subscriber::builder().finish();
    // attempt to set, failure means already set (other test suite, likely)
    let _ = tracing::subscriber::set_global_default(subscriber);
}
