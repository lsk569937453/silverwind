#[macro_use] extern crate rocket;
mod control_plane;
mod proxy;
mod pool;
mod timer;
use std::env;
use proxy::HttpProxy;
#[macro_use]
extern crate log;
#[launch]
fn rocket() -> _ {
    env::set_var("RUST_LOG", "debug");
    env::set_var("ROCKET_PORT","3721");

    env_logger::init();

    tokio::spawn(async move{
        startProxy().await;
    });
    tokio::spawn(async move{
        timer::startSyncTask().await;
    });
    
    rocket::build()
    .attach(control_plane::stage())
}
pub async fn startProxy(){
    let inBound:String=String::from("Hello, world!");
    let outBound:String=String::from("Hello, world!");

    let mut httpProxy=HttpProxy{inBound:inBound,outBound:outBound};
    
    httpProxy.start().await;
}