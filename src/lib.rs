
#[cfg(test)]
// NOTE: takes Fn b/c it may use tokio::spawn and needs to have a runtime ready
// TODO: could take list of futures and be top level test runner.. (better than one tokio runtime per test case)
pub fn run_test<F, E>(f: F)
where
    F: Fn() -> BoxFuture<(), E> + Send + Sync + 'static,
    E: std::fmt::Debug + Send + Sync + 'static,
{
    // initialize and register event/span logging subscriber
    let subscriber = tracing_subscriber::fmt::Subscriber::builder().finish();
    // attempt to set, failure means already set (other test suite?)
    let _ = tracing::subscriber::set_global_default(subscriber);

    let f = futures01::future::ok(())
        .and_then(move |()| f())
        .then(|res| {
            match res {
                Err(err) => {
                    // NOTE: had problems communicating failure outwards, just nuke it all and panic on failure
                    panic!("test failed, err: {:?}", err)
                }
                Ok(_) => {
                    println!("test passed, yay") // doing this b/c panic above doesn't propagate as expected
                }
            }
            futures01::future::ok(())
        });

    tokio::run(f);
}
