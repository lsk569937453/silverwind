#[macro_use]
extern crate rocket;
#[macro_use]
extern crate anyhow;
extern crate derive_builder;
mod configuration_service;
mod control_plane;
mod dao;
mod pool;
mod proxy;
mod timer;
mod vojo;
use std::env;
mod constants;
use tokio::runtime::Handle;
#[macro_use]
extern crate log;
#[launch]
fn rocket() -> _ {
    env::set_var("RUST_BACKTRACE", "1");
    env::set_var("RUST_LOG", "debug");
    env::set_var(
        "DATABASE_URL",
        "mysql://axway:axway-password@127.0.0.1:3306/yyproxy",
    );

    env_logger::init();
    tokio::task::block_in_place(move || {
        Handle::current().block_on(async {
            configuration_service::app_config_servive::init().await;
        })
    });

    std::thread::spawn(move || {
        pool::pgpool::schedule_task_connection_pool();
    });

    rocket::build().attach(control_plane::stage())
}

pub fn test() {
    let path = String::from("/Users/sliu/code/github/GoProxy/yaml/petstore.yaml");
    match oas3::from_path(path) {
        Ok(spec) => {
            debug!("spec: {:?}", spec);
            debug!("pathWithMethod:{:?}", spec.paths)
        }
        Err(err) => error!("error: {}", err),
    }
}
