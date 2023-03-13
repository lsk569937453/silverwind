#[macro_use]
extern crate rocket;
#[macro_use]
extern crate anyhow;
use std::env;
extern crate derive_builder;
mod configuration_service;
mod constants;
mod control_plane;
mod proxy;
mod vojo;
use std::net::TcpListener;
use tokio::runtime::Handle;
#[macro_use]
extern crate log;
#[launch]
fn rocket() -> _ {
    env::set_var("RUST_BACKTRACE", "1");
    env::set_var("RUST_LOG", "debug");
    env_logger::init();
    tokio::task::block_in_place(move || {
        Handle::current().block_on(async {
            configuration_service::app_config_service::init().await;
        })
    });
    rocket::build().attach(control_plane::stage())
}
