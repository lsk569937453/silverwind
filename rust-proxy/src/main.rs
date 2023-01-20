#![warn(rust_2018_idioms)]

use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

use async_trait::async_trait;
use futures::FutureExt;
use std::error::Error;
use std::string::ParseError;
use std::{env, fmt, usize};

use bb8::{ManageConnection, Pool};
#[derive(Debug)]
pub struct TcpConnectionManager {
    addr: String,
}

impl TcpConnectionManager {
    /// Create a new `RedisConnectionManager`.
    /// See `redis::Client::open` for a description of the parameter types.
    fn new(info: String) -> Result<TcpConnectionManager, MyError> {
        Ok(TcpConnectionManager { addr: info })
    }
}
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

#[async_trait]
impl bb8::ManageConnection for TcpConnectionManager {
    type Connection = TcpStream;
    type Error = MyError;

    async fn connect(&self) -> Result<Self::Connection, Self::Error> {
        match TcpStream::connect(self.addr.clone()).await {
            Ok(tcpStream) => Ok(tcpStream),
            Err(err) => return Err(MyError::new("ssss")),
        }
    }

    async fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        let mut b1 = [0; 1];
        let peek_result = conn.peek(&mut b1).await;
        match peek_result {
            Ok(1usize) => Ok(()),
            _ => return Err(MyError::new("ssss")),
        }
        // let pong: String = redis::cmd("PING").query_async(conn).await?;
        // match pong.as_str() {
        //     "PONG" => Ok(()),
        //     _ => Err((ErrorKind::ResponseError, "ping request").into()),
        // }
    }

    fn has_broken(&self, _: &mut Self::Connection) -> bool {
        false
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let manager = TcpConnectionManager::new("localhost:9888".to_string()).unwrap();
    let mut pool = bb8::Pool::builder()
        .max_size(20)
        .build(manager)
        .await
        .unwrap();

    let listen_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:9950".to_string());
    let server_addr = env::args()
        .nth(2)
        .unwrap_or_else(|| "127.0.0.1:8888".to_string());

    println!("Listening on: {}", listen_addr);
    println!("Proxying to: {}", server_addr);

    let listener = TcpListener::bind(listen_addr).await?;
    // let accept_result=listener.accept().await;
    // while (true) {
    // match accept_result {
    //     Ok(( inbound,_))=>{
    //         let transfer = transfer(inbound, server_addr.clone(),pool.clone()).map(|r| {
    //             if let Err(e) = r {
    //                 println!("Failed to transfer; error={}", e);
    //             }
    //         });

    //         tokio::spawn(transfer);
    //     }
    //     Err(err)=>{println!("{}", err);}
    // }
    // }
    while let accept_result = listener.accept().await {
        match accept_result {
            Ok((inbound, _)) => {
                let transfer = transfer(inbound, server_addr.clone(), pool.clone()).map(|r| {
                    if let Err(e) = r {
                        println!("Failed to transfer; error={}", e);
                    }
                });

                tokio::spawn(transfer);
            },
            Err(err) => {
                println!("{}", err);
            },
        }
    }

    
    // while let Ok((inbound, _)) = listener.accept().await {
    //     let transfer = transfer(inbound, server_addr.clone(),pool.clone()).map(|r| {
    //         if let Err(e) = r {
    //             println!("Failed to transfer; error={}", e);
    //         }
    //     });

    //     tokio::spawn(transfer);
    // }

    println!("after catch_unwind");

    Ok(())
}

async fn transfer(
    mut inbound: TcpStream,
    proxy_addr: String,
    pool: Pool<TcpConnectionManager>,
) -> Result<(), Box<dyn Error>> {
    let mut outbound = pool.get().await.unwrap();
    let (mut ri, mut wi) = inbound.split();
    let (mut ro, mut wo) = outbound.split();

    let client_to_server = async {
        io::copy(&mut ri, &mut wo).await
        // wo.shutdown().await
    };

    let server_to_client = async {
        io::copy(&mut ro, &mut wi).await
        // wi.shutdown().await
    };

    tokio::try_join!(client_to_server, server_to_client)?;

    Ok(())
}
