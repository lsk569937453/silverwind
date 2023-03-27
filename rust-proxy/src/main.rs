#[macro_use]
extern crate anyhow;
extern crate derive_builder;
mod configuration_service;
mod constants;
mod control_plane;
mod proxy;
mod vojo;
#[macro_use]
extern crate log;
use crate::control_plane::control_plane::start_control_plane;
use tokio::runtime;

fn main() {
    let rt = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let start_result = rt.block_on(async { start(8870).await });
    match start_result {
        Ok(_) => info!("start successfully!"),
        Err(err) => error!("{}", err.to_string()),
    }
}
async fn start(api_port: i32) -> Result<(), hyper::Error> {
    configuration_service::app_config_service::init().await;
    start_control_plane(api_port).await
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
            let res = start(8870).await;
            match res {
                Ok(_) => println!(""),
                Err(err) => println!("{}", err.to_string()),
            }
        });
        let sleep_time = time::Duration::from_millis(1000);
        thread::sleep(sleep_time);
        let listener = TcpListener::bind("127.0.0.1:8870");
        assert_eq!(listener.is_err(), true);
    }
    #[test]
    fn test_start_api_error() {
        let _listener = TcpListener::bind("127.0.0.1:8860");
        TOKIO_RUNTIME.spawn(async {
            let res = start(8860).await;
            assert_eq!(res.is_err(), true);
        });
        let sleep_time = time::Duration::from_millis(20);
        thread::sleep(sleep_time);
    }
}
