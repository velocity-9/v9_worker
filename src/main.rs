// I'd like the most pedantic warning level
#![warn(clippy::pedantic, clippy::needless_borrow)]
// But I don't care about these ones for now (most applicable since the code isn't fleshed out)
#![allow(
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]

#[macro_use]
extern crate failure;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde;

mod component;
mod error;
mod model;
mod named_pipe;
mod request_handler;
mod server;

use std::env;
use std::sync::Arc;

use crate::request_handler::HttpRequestHandler;

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

    // Create handler to deal with HTTP requests
    let http_request_handler = HttpRequestHandler::new();

    server::start_server(
        development_mode,
        Arc::new(http_request_handler),
        request_handler::global_request_entrypoint,
    );

    info!("Sever loop finished, shutting down...");
}
