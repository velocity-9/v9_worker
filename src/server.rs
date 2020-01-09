use std::convert::Infallible;
use std::error::Error;
use std::future::Future;
use std::sync::Arc;

use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use tokio::runtime::Runtime;
use tokio::spawn;

const PRODUCTION_PORT: u16 = 80;
const DEVELOPMENT_PORT: u16 = 8082;

pub fn start_server<S, E, F>(
    development_mode: bool,
    state: Arc<S>,
    handler: fn(Arc<S>, Request<Body>) -> F,
) where
    S: Send + Sync + 'static,
    E: Error + Send + Sync + 'static,
    F: Future<Output = Result<Response<Body>, E>> + Send + 'static,
{
    Runtime::new().expect("Only should be called from main").block_on(async {
        let port = if development_mode {
            DEVELOPMENT_PORT
        } else {
            PRODUCTION_PORT
        };

        let addr = ([0, 0, 0, 0], port).into();
        info!("Spinning up server on {:?}", addr);

        let new_service = make_service_fn(move |_| {
            let copied_state = state.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| handler(copied_state.clone(), req)))
            }
        });

        let server = Server::bind(&addr)
            .serve(new_service);

        spawn(server)
            .await
            .expect("Server should be created successfully")
            .expect("Our service is infallible");
    });
}
