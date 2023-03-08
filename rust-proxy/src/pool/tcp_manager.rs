use async_trait::async_trait;
use lazy_static::lazy_static;
use log::{debug, error};
use std::error::Error;
use std::fmt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

#[derive(Clone, Debug)]
pub struct TcpConnectionManager {
    backend_url: String,
}

impl TcpConnectionManager {
    pub fn new(backend_url: String) -> Result<TcpConnectionManager, anyhow::Error> {
        Ok(TcpConnectionManager {
            backend_url: backend_url,
        })
    }
}

#[async_trait]
impl bb8::ManageConnection for TcpConnectionManager {
    type Connection = TcpStream;
    type Error = anyhow::Error;
    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
        debug!("start connect");
        return match TcpStream::connect(self.backend_url.clone()).await {
            Ok(tcp_stream) => Ok(tcp_stream),
            Err(err) => {
                error!("connect error,error is{}", err);
                Err(anyhow!(err.to_string()))
            }
        };
    }

    async fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        debug!("peek start");
        let (mut ro, mut wo) = conn.split();
        // return wo.writable();

        match wo.writable().await {
            Ok(n) => n,
            Err(err) => {
                debug!("can not write the");
                return Err(anyhow!(err.to_string()));
            }
        };
        match ro.readable().await {
            Ok(n) => n,
            Err(err) => {
                debug!("can not read the");
                return Err(anyhow!(err.to_string()));
            }
        };
        debug!("peek successfully");
        Ok(())
    }

    fn has_broken(&self, conn: &mut Self::Connection) -> bool {
        debug!("has_broken start");
        return false;
    }
}
mod tests {
    use super::*;
    use std::io::{BufReader, Read};
    use tokio::runtime::Handle;
    use tokio::runtime::{Builder, Runtime};

    use bb8::{Pool, RunError};
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
    fn test_pool() {
        TOKIO_RUNTIME.block_on(async move {
            let backend_url = String::from("127.0.0.1:3306");
            let manager = TcpConnectionManager::new(backend_url).unwrap();

            let pool = Pool::builder()
                .min_idle(Some(5))
                .max_size(10)
                .build(manager)
                .await
                .unwrap();
        });
    }
}
