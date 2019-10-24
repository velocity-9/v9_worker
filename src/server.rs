use std::sync::Arc;

use hyper::rt::{self, Future};
use hyper::service::service_fn;
use hyper::{Body, Request, Response, Server};

const PRODUCTION_PORT: u16 = 80;
const DEVELOPMENT_PORT: u16 = 8082;

pub fn start_server<S, F>(development_mode: bool, state: Arc<S>, handler: fn(Arc<S>, Request<Body>) -> F)
where
    S: Send + Sync + 'static,
    F: Future<Item = Response<Body>, Error = hyper::error::Error> + Send + 'static,
{
    let port = if development_mode {
        DEVELOPMENT_PORT
    } else {
        PRODUCTION_PORT
    };

    let addr = ([0, 0, 0, 0], port).into();
    info!("Spinning up server on {:?}", addr);

    let new_service = move || {
        let copied_state = state.clone();
        service_fn(move |req| handler(copied_state.clone(), req))
    };

    let server = Server::bind(&addr)
        .serve(new_service)
        .map_err(|e| error!("server error: {}", e));

    rt::run(server);
}
