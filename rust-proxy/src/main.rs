#[cfg(not(target_env = "msvc"))]
use jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

#[macro_use]
extern crate anyhow;
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
    configuration_service::app_config_service::init().await;
    start_control_plane(admin_port).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazy_static::lazy_static;
    use std::net::TcpListener;
    use std::{thread, time};
    use tokio::runtime::{Builder, Runtime};
    lazy_static! {
        pub static ref TOKIO_RUNTIME: Runtime = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("silverwind-thread")
            .thread_stack_size(3 * 1024 * 1024)
            .enable_all()
            .build()
            .unwrap();
    }
    #[test]
    fn test_start_api_ok() {
        TOKIO_RUNTIME.spawn(async {
            start(5402).await;
        });
        let sleep_time = time::Duration::from_millis(5000);
        thread::sleep(sleep_time);
        let listener = TcpListener::bind("127.0.0.1:5402");
        assert!(listener.is_err());
    }
}
