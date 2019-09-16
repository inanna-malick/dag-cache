use futures::future::Future;

pub type BoxFuture<Item, Error> = Box<dyn Future<Item = Item, Error = Error> + 'static + Send>;
