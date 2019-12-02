extern crate tiny_web;
extern crate env_logger;

use std::sync::Arc;
use std::thread;

fn main() {
    env_logger::init();
    println!("web");
    let server = Arc::new(tiny_web::Server::http("0.0.0.0:9876").unwrap());
    println!("Now listening on port 9876");

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
