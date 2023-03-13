#[macro_use]
extern crate anyhow;
use std::env;
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
    env::set_var("RUST_BACKTRACE", "1");
    env::set_var("RUST_LOG", "debug");
    env_logger::init();
    let rt = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let start_result = rt.block_on(async {
        configuration_service::app_config_service::init().await;
        start_control_plane().await
    });
    match start_result {
        Ok(_) => info!("start successfully!"),
        Err(err) => error!("{}", err.to_string()),
    }
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
    fn test_output_serde() {
        TOKIO_RUNTIME.spawn(async {
            let res = start_control_plane().await;
            match res {
                Ok(_) => println!(""),
                Err(err) => println!("{}", err.to_string()),
            }
        });
        let ten_millis = time::Duration::from_millis(20);
        thread::sleep(ten_millis);
        let listener = TcpListener::bind("127.0.0.1:8870");
        assert_eq!(listener.is_err(), true);
    }
}
