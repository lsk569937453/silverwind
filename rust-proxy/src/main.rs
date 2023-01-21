#![warn(rust_2018_idioms)]

use bb8::Pool;
use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

use async_trait::async_trait;
use futures::FutureExt;
use log::{debug, error, info, log_enabled, Level};
use std::env;
use std::error::Error;
use std::fmt;
use std::task::Poll;
use backtrace::Backtrace;

#[derive(Debug)]
pub struct MyError {
    details: String,
}

impl MyError {
    fn new(msg: &str) -> MyError {
        MyError {
            details: msg.to_string(),
        }
    }
}

impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

impl Error for MyError {
    fn description(&self) -> &str {
        &self.details
    }
}
#[derive(Clone, Debug)]
pub struct TcpConnectionManager {
    backendUrl: String,
}

impl TcpConnectionManager {
    /// Create a new `RedisConnectionManager`.
    /// See `redis::Client::open` for a description of the parameter types.
    pub fn new(info: String) -> Result<TcpConnectionManager, MyError> {
        Ok(TcpConnectionManager { backendUrl: info })
    }
}

#[async_trait]
impl bb8::ManageConnection for TcpConnectionManager {
    type Connection = TcpStream;
    type Error = MyError;

    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
        match TcpStream::connect(self.backendUrl.clone()).await {
            Ok(tcpStream) => Ok(tcpStream),
            Err(err) => Err(MyError::new(err.to_string().as_str())),
        }
    }

    async fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        let mut b1 = [0; 1];
        conn.peek(&mut b1).await;
        Ok(())
    }

    fn has_broken(&self, _: &mut Self::Connection) -> bool {
        false
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env::set_var("RUST_LOG", "debug");

    env_logger::init();

    let listen_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8081".to_string());
    let server_addr = env::args()
        .nth(2)
        .unwrap_or_else(|| "httpbin.org:80".to_string());

    info!("Listening on: {}", listen_addr);
    info!("Proxying to: {}", server_addr);

    let manager = TcpConnectionManager::new(server_addr).unwrap();
    let pool = bb8::Pool::builder().build(manager).await.unwrap();

    let listener = TcpListener::bind(listen_addr).await?;

    while let Ok((inbound, _)) = listener.accept().await {
        let transfer = transfer(inbound,pool.clone()).map(|r| {
            if let Err(e) = r {
                let bt = Backtrace::new();
                error!("Failed to transfer; error={},stack is:{:?}", e,bt);
            }
        });

        tokio::spawn(transfer);
    }

    Ok(())
}

async fn transfer(mut inbound: TcpStream,pool:Pool<TcpConnectionManager>) -> Result<(), Box<dyn Error>> {
    // let mut outbound = TcpStream::connect(proxy_addr).await?;
    let mut outbound=pool.get().await.unwrap();
    let (mut ri, mut wi) = inbound.split();
    let (mut ro, mut wo) = outbound.split();

    let client_to_server = async {
        io::copy(&mut ri, &mut wo).await?;
        wo.shutdown().await
    };

    let server_to_client = async {
        io::copy(&mut ro, &mut wi).await?;
        wi.shutdown().await
    };

    tokio::try_join!(client_to_server, server_to_client)?;

    Ok(())
}
