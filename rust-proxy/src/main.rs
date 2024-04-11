#[cfg(not(target_env = "msvc"))]
use jemallocator::Jemalloc;
use vojo::handler::Handler;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

extern crate derive_builder;
mod configuration_service;
mod constants;
mod control_plane;
mod health_check;
mod monitor;
mod proxy;
mod utils;
mod vojo;
use crate::constants::common_constants::DEFAULT_ADMIN_PORT;
use crate::constants::common_constants::ENV_ADMIN_PORT;
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex;
#[macro_use]
extern crate log;
use crate::control_plane::rest_api::start_control_plane;
use env_logger::Env;

use tokio::runtime;

fn main() {
    let num = num_cpus::get();
    let rt = runtime::Builder::new_multi_thread()
        .worker_threads(num * 2)
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        let admin_port: i32 = env::var(ENV_ADMIN_PORT)
            .unwrap_or(String::from(DEFAULT_ADMIN_PORT))
            .parse()
            .unwrap();
        start(admin_port).await
    });
}
async fn start(admin_port: i32) {
    let mut handler = Handler {
        shared_app_config: Arc::new(Mutex::new(Default::default())),
    };
    // configuration_service::app_config_service::init().await;
    // start_control_plane(admin_port).await;
    handler.run(admin_port).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::{thread, time};
    use tokio::runtime::{Builder, Runtime};
}
