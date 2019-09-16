use std::env;

mod request_handler;
mod server;

fn main() {
    let development_mode = env::args().any(|arg| arg == "--development");

    server::start_server(development_mode, request_handler::request_entrypoint)
}
