#[macro_use]
extern crate log;
extern crate env_logger;
extern crate tiny_web;

use std::sync::Arc;
use std::thread;

fn main() {
    env_logger::init();
    let version = env!("CARGO_PKG_VERSION");
    let server = Arc::new(tiny_web::Server::http("0.0.0.0:9876").unwrap());
    info!("Now listening on port 9876, version: {}", version);

    let mut handles = Vec::new();
    for _ in 0..4 {
        let server = server.clone();

        handles.push(thread::spawn(move || {
            for req in server.incoming_requests() {
                let response = tiny_web::Response::from_string("hello world".to_string());
                let _ = req.respond(response);
            }
        }))
    }

    for h in handles {
        h.join().unwrap();
    }
}
