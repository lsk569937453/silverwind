mod proxy;
mod pool;
use std::env;
use proxy::HttpProxy;
#[macro_use]
extern crate log;
// To try this example:
// 1. cargo run --example http_proxy
// 2. config http_proxy in command line
//    $ export http_proxy=http://127.0.0.1:8100
//    $ export https_proxy=http://127.0.0.1:8100
// 3. send requests
//    $ curl -i https://www.some_domain.com/
#[tokio::main]
async fn main(){
    env::set_var("RUST_LOG", "debug");

    env_logger::init();

    let inBound:String=String::from("Hello, world!");
    let outBound:String=String::from("Hello, world!");

    let mut httpProxy=HttpProxy{inBound:inBound,outBound:outBound};
    httpProxy.start().await;
    
}

// async fn proxy(client: HttpClient, mut req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
//     debug!("req: {:?}", req);
    
//     // *req.uri_mut() = "http://host.docker.internal:8888/get".parse().unwrap();
//     *req.uri_mut() = "http://httpbin.org:80/get".parse().unwrap();

    
//     if Method::CONNECT == req.method() {
//         // Received an HTTP request like:
//         // ```
//         // CONNECT www.domain.com:443 HTTP/1.1
//         // Host: www.domain.com:443
//         // Proxy-Connection: Keep-Alive
//         // ```
//         //
//         // When HTTP method is CONNECT we should return an empty body
//         // then we can eventually upgrade the connection and talk a new protocol.
//         //
//         // Note: only after client received an empty body with STATUS_OK can the
//         // connection be upgraded, so we can't return a response inside
//         // `on_upgrade` future.
//         if let Some(addr) = host_addr(req.uri()) {
//             tokio::task::spawn(async move {
//                 match hyper::upgrade::on(req).await {
//                     Ok(upgraded) => {
//                         if let Err(e) = tunnel(upgraded, addr).await {
//                             error!("server io error: {}", e);
//                         };
//                     }
//                     Err(e) => error!("upgrade error: {}", e),
//                 }
//             });

//             Ok(Response::new(Body::empty()))
//         } else {
//             info!("CONNECT host is not socket addr: {:?}", req.uri());
//             let mut resp = Response::new(Body::from("CONNECT must be to a socket address"));
//             *resp.status_mut() = http::StatusCode::BAD_REQUEST;

//             Ok(resp)
//         }
//     } else {
//         client.request(req).await
//     }
// }

// fn host_addr(uri: &http::Uri) -> Option<String> {
//     uri.authority().and_then(|auth| Some(auth.to_string()))
// }

// // Create a TCP connection to host:port, build a tunnel between the connection and
// // the upgraded connection
// async fn tunnel(mut upgraded: Upgraded, addr: String) -> std::io::Result<()> {
//     // Connect to remote server
//     let mut server = TcpStream::connect(addr).await?;

//     // Proxying data
//     let (from_client, from_server) =
//         tokio::io::copy_bidirectional(&mut upgraded, &mut server).await?;

//     // Print message when done
//     info!(
//         "client wrote {} bytes and received {} bytes",
//         from_client, from_server
//     );

//     Ok(())
// }
