use futures::future::Future;

pub type BoxFuture<Item, Error> = Box<dyn Future<Item = Item, Error = Error> + 'static + Send>;

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

    let (send, receive) = futures::sync::oneshot::channel();
    let f = futures::future::ok(()).and_then(move |()| f()).then(|res| {
        let chan_send_res = send.send(res);
        if let Err(err) = chan_send_res {
            println!("failed oneshot channel send {:?}", err);
        };

        Ok(())
    });

    tokio::run(f);

    if let Err(e) = receive.wait().unwrap() {
        println!("test failed with error {:?}", e);
    };
}
