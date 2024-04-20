#[cfg(not(target_env = "msvc"))]
use jemallocator::Jemalloc;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use vojo::app_error::AppError;
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
use futures::Future;
use std::env;
use std::sync::Arc;
use tokio::sync::Mutex;

#[macro_use]
extern crate log;

use tokio::runtime;
fn main() {
    let fut = async move { start().await };
    if let Err(e) = main_with_error(fut) {
        error!("main error,the error is {}!", e)
    }
}
fn main_with_error<Fut>(fut: Fut) -> Result<(), AppError>
where
    Fut: Future<Output = ()> + Send + 'static,
{
    let num = num_cpus::get();
    let rt = runtime::Builder::new_multi_thread()
        .worker_threads(num * 2)
        .enable_all()
        .build()
        .map_err(|e| AppError(e.to_string()))?;
    rt.block_on(fut);
    Ok(())
}
async fn start() {
    let admin_port: i32 = env::var(ENV_ADMIN_PORT)
        .unwrap_or(String::from(DEFAULT_ADMIN_PORT))
        .parse()
        .unwrap();
    let mut handler = Handler {
        shared_app_config: Arc::new(Mutex::new(Default::default())),
    };
    let _ = handler.run(admin_port).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Duration;
    use tokio::time::sleep;
    #[tokio::test]
    async fn pool_key_value_get_set() {
        tokio::spawn(async move { start().await });
        sleep(Duration::from_millis(1000)).await;
        let listener = TcpListener::bind("0.0.0.0:5402");
        assert!(listener.is_ok());
    }
    #[test]
    fn test_main_success() {
        let res = main_with_error(async move {
            let a = 1;
        });
        assert!(res.is_ok());
    }
}
