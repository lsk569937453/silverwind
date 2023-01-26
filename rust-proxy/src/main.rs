mod proxy;
mod pool;
use std::env;
use proxy::HttpProxy;
#[macro_use]
extern crate log;
#[tokio::main]
async fn main(){
    env::set_var("RUST_LOG", "debug");

    env_logger::init();

    let inBound:String=String::from("Hello, world!");
    let outBound:String=String::from("Hello, world!");

    let mut httpProxy=HttpProxy{inBound:inBound,outBound:outBound};
    httpProxy.start().await;
    
}
