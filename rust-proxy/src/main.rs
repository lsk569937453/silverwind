#[macro_use]
extern crate anyhow;
extern crate derive_builder;
mod configuration_service;
mod constants;
mod control_plane;
mod monitor;
mod proxy;
mod vojo;
use std::env;
#[macro_use]
extern crate log;
use crate::control_plane::control_plane::start_control_plane;
use tokio::runtime;

fn main() {
    let rt = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        let admin_port: i32 = env::var("ADMIN_PORT")
            .unwrap_or(String::from("8870"))
            .parse()
            .unwrap();
        start(admin_port).await
    });
}
async fn start(api_port: i32) {
    configuration_service::app_config_service::init().await;
    start_control_plane(api_port).await;
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
            .thread_name("my-custom-name")
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
        let sleep_time = time::Duration::from_millis(2000);
        thread::sleep(sleep_time);
        let listener = TcpListener::bind("127.0.0.1:5402");
        assert_eq!(listener.is_err(), true);
    }
    #[test]
    fn test_start_api_error() {
        let _listener = TcpListener::bind("127.0.0.1:8860");
        TOKIO_RUNTIME.spawn(async {
            let _res = start(8860).await;
            // assert_eq!(res.is_err(), true);
        });
        let sleep_time = time::Duration::from_millis(20);
        thread::sleep(sleep_time);
    }
}
