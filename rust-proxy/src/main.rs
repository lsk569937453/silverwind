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
use proxy::HttpProxy;
use std::env;
use futures::executor::block_on;

#[macro_use]
extern crate log;
#[launch]
fn rocket() -> _ {
    env::set_var("RUST_BACKTRACE", "1");
    env::set_var("RUST_LOG", "debug");
    env::set_var("ROCKET_PORT", "3721");
    env::set_var(
        "DATABASE_URL",
        // "postgresql://axway:axway-password@127.0.0.1:6543/yyproxy",
        "mysql://axway:axway-password@127.0.0.1:3306/yyproxy",
    );

    env_logger::init();

    tokio::task::spawn(async{start_proxy().await});
    std::thread::spawn(move||{pool::pgpool::schedule_task_connection_pool();});

   
    rocket::build().attach(control_plane::stage())
}
pub async fn start_proxy() {
    let in_bound: String = String::from("Hello, world!");
    let out_bound: String = String::from("Hello, world!");

    let http_proxy = HttpProxy {
        in_bound: in_bound,
        out_bound: out_bound,
    };

    http_proxy.start().await;
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
