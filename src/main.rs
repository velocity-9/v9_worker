#[macro_use]
extern crate failure;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde;

use std::env;

mod components;
mod error;
mod model;
mod request_handler;
mod server;

fn main() {
    // Initialize logging
    flexi_logger::Logger::with_str("debug, tokio_reactor=info, hyper=info")
        .start()
        .unwrap();
    info!("worker starting... (logging initialized)");

    // Parse command line arguments
    let development_mode = env::args().any(|arg| arg == "--development");
    if development_mode {
        info!("running in development mode");
    }

    // We don't want to hang the first REST call initializing our GLOBAL_HANDLER, so pre-initialize it
    lazy_static::initialize(&request_handler::GLOBAL_HANDLER);

    server::start_server(development_mode, request_handler::global_request_entrypoint);

    info!("Sever loop finished, shutting down...");
}
