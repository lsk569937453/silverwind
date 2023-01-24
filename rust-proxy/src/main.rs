#![warn(rust_2018_idioms)]

mod pool;

use futures::FutureExt;
use tokio::io::{self, AsyncReadExt};
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
use futures::StreamExt;
use tokio_util::codec::{BytesCodec, Decoder};
use tokio_util::codec::{ Encoder, Framed};
use std::io::BufReader;
use std::io::{Write, BufRead};

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


    let mut framed = BytesCodec::new().framed(&mut inbound);
    debug!("after get");

    let message=framed.next().await.unwrap().unwrap();
    debug!("framed next end");

    outbound.write_all( &message[..]).await;
    debug!("outbound write all");


    let mut reader = BufReader::new( outbound.into_std().unwrap());

    loop {
        let mut response = String::new();

        let result = reader.read_line(&mut response);
        match result {
            Ok(data) => {
                println!("{}", response);
            }
            Err(e) => {
                println!("error reading: {}", e);
                break;
            }
        }
    }


    // let mut framed2 = BytesCodec::new().framed(&mut outbound);

    // let mut secondBuffer=framed2.next().await.unwrap().unwrap();

    // inbound.write_all(&secondBuffer[..]).await;
    debug!("inbound write_all");

    Ok(())

}
