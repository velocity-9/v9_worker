use hyper::rt::{self, Future};
use hyper::service::service_fn_ok;
use hyper::{Body, Request, Response, Server};

const PRODUCTION_PORT: u16 = 80;
const DEVELOPMENT_PORT: u16 = 8082;

pub fn start_server(development_mode: bool, handler: fn(Request<Body>) -> Response<Body>) {
    let port = if development_mode {
        DEVELOPMENT_PORT
    } else {
        PRODUCTION_PORT
    };

    let addr = ([127, 0, 0, 1], port).into();

    let new_service = move || service_fn_ok(handler);

    let server = Server::bind(&addr)
        .serve(new_service)
        .map_err(|e| eprintln!("server error: {}", e));

    rt::run(server);
}
