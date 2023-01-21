#![warn(rust_2018_idioms)]

mod pool;

use futures::FutureExt;
use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

use backtrace::Backtrace;
use bb8::Pool;
use futures::future;
use log::{debug, error, info, Level};
use pool::TcpConnectionManager;
use std::env;
use std::error::Error;
use std::fmt;

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
    let pool = bb8::Pool::builder()
        .max_size(3)
        .build(manager)
        .await
        .unwrap();

    let listener = TcpListener::bind(listen_addr).await?;

    while let Ok((inbound, _)) = listener.accept().await {
        let transfer = transfer(inbound, pool.clone()).map(|r| {
            if let Err(e) = r {
                let bt = Backtrace::new();
                error!("Failed to transfer; error={},stack is:{:?}", e, bt);
            }
        });

        tokio::spawn(transfer);
    }

    Ok(())
}

async fn transfer(
    mut inbound: TcpStream,
    pool: Pool<TcpConnectionManager>,
) -> Result<(), Box<dyn Error>> {
    // let mut outbound = TcpStream::connect(proxy_addr).await?;
    debug!("before get");
    let mut outbound = pool.get().await.unwrap();
    debug!("after get");

    let (mut ri, mut wi) = inbound.split();
    let (mut ro, mut wo) = outbound.split();

    let client_to_server = async {
        match io::copy(&mut ri, &mut wo).await{
            Ok(len)=>{info!("client_to_server,copy data:{}",len)},
            Err(_)=>{info!("errr")},
        }
        debug!("close  wo");
        let a = future::ok::<i32, pool::MyError>(1);
        a.await
        //   wo.shutdown().await
    };

    let server_to_client = async {
        match io::copy(&mut ro, &mut wi).await{
            Ok(len)=>{info!("server_to_client,copy data:{}",len)},
            Err(_)=>{info!("errr")},
        }
        debug!("close  wi");
        //  wi.shutdown().await;
        let a = future::ok::<i32, pool::MyError>(1);
        a.await
    };

    tokio::try_join!(client_to_server, server_to_client)?;
    debug!("try_join over");

    Ok(())
}
