// I'd like the most pedantic warning level
#![warn(
    clippy::cargo,
    clippy::needless_borrow,
    clippy::pedantic,
    clippy::redundant_clone
)]
// But I don't care about these ones
#![allow(
    clippy::cast_precision_loss,     // There is no way to avoid this precision loss
    clippy::module_name_repetitions, // Sometimes clear naming calls for repetition
    clippy::multiple_crate_versions  // There is no way to easily fix this without modifying our dependencies
)]

#[macro_use]
extern crate failure;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde;

mod component;
mod docker;
mod error;
mod fs_utils;
mod model;
mod named_pipe;
mod request_handler;
mod server;

use std::env;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::request_handler::HttpRequestHandler;

const HEARTBEAT_PERIODICITY: Duration = Duration::from_secs(1);

fn main() {
    // TODO: Graceful shutdown on control-c / API call would be good

    // Initialize logging
    let log_spec = "debug, hyper=info, mio=info, tokio_reactor=info, tokio_threadpool=info";
    flexi_logger::Logger::with_str(log_spec).start().unwrap();
    info!("worker starting... (logging initialized)");

    // Parse command line arguments
    let development_mode = env::args().any(|arg| arg == "--development");
    if development_mode {
        info!("running in development mode");
    }

    // Pre-initialize idle container creation
    lazy_static::initialize(&docker::idle_container_creator::GLOBAL_IDLE_CONTAINER_CREATOR);

    // Create handler to deal with HTTP requests
    let http_request_handler = Arc::new(HttpRequestHandler::new());

    // Create a heartbeat thread for the ComponentManager
    // (We want a periodic signal to check on our components, and perhaps shut them down)
    let heartbeat_handler_ref = http_request_handler.clone();
    thread::spawn(move || loop {
        heartbeat_handler_ref.component_manager().read().heartbeat();
        thread::sleep(HEARTBEAT_PERIODICITY);
    });

    // Start up a server to respond to REST requests
    server::start_server(
        development_mode,
        http_request_handler,
        request_handler::global_request_entrypoint,
    );

    warn!("Sever loop finished, shutting down...");
}
