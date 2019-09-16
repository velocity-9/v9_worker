#[macro_use]
extern crate lazy_static;

use std::env;

mod request_handler;
mod server;

fn main() {
    let development_mode = env::args().any(|arg| arg == "--development");

    // We don't want to hang the first REST call initializing our GLOBAL_HANDLER, so pre-initialize it
    lazy_static::initialize(&request_handler::GLOBAL_HANDLER);

    server::start_server(development_mode, request_handler::global_request_entrypoint)
}
