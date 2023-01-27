#[macro_use] extern crate rocket;
mod control_plane;
use control_plane::service_controller;
use std::env;
#[macro_use]
extern crate log;
#[launch]
fn rocket() -> _ {
    env::set_var("RUST_LOG", "debug");
    env::set_var("ROCKET_PORT","3721");

    env_logger::init();

    rocket::build()
    .attach(service_controller::stage())
}