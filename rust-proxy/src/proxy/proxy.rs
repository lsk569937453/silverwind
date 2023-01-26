use crate::pool::MyError;
use hyper::upgrade::Upgraded;
use hyper::{Body, Client, Method, Request, Response, Server};
// use pool::MyError;
use std::convert::Infallible;
use std::net::SocketAddr;
use tokio::net::TcpStream;

use hyper::service::{make_service_fn, service_fn};

type HttpClient = Client<hyper::client::HttpConnector>;

#[derive(Clone, Debug)]
pub struct HttpProxy {
    pub inBound: String,
    pub outBound: String,
}

impl HttpProxy {
    /// Create a new `RedisConnectionManager`.
    /// See `redis::Client::open` for a description of the parameter types.
    pub async fn start(&self)  {
        let addr = SocketAddr::from(([0, 0, 0, 0], 9550));

        let client = Client::builder()
            .http1_title_case_headers(true)
            .http1_preserve_header_case(true)
            .build_http();

        let make_service = make_service_fn(move |_| {
            let client = client.clone();
            async move { Ok::<_, Infallible>(service_fn(move |req| proxy(client.clone(), req))) }
        });

        let server = Server::bind(&addr)
            .http1_preserve_header_case(true)
            .http1_title_case_headers(true)
            .serve(make_service);

        info!("Listening on http://{}", addr);

        if let Err(e) = server.await {
            error!("server error: {}", e);
        }
       
    }
}
async fn proxy(client: HttpClient, mut req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    debug!("req: {:?}", req);

    // *req.uri_mut() = "http://host.docker.internal:8888/get".parse().unwrap();
    *req.uri_mut() = "http://httpbin.org:80/get".parse().unwrap();

    if Method::CONNECT == req.method() {
        // Received an HTTP request like:
        // ```
        // CONNECT www.domain.com:443 HTTP/1.1
        // Host: www.domain.com:443
        // Proxy-Connection: Keep-Alive
        // ```
        //
        // When HTTP method is CONNECT we should return an empty body
        // then we can eventually upgrade the connection and talk a new protocol.
        //
        // Note: only after client received an empty body with STATUS_OK can the
        // connection be upgraded, so we can't return a response inside
        // `on_upgrade` future.
        if let Some(addr) = host_addr(req.uri()) {
            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Err(e) = tunnel(upgraded, addr).await {
                            error!("server io error: {}", e);
                        };
                    }
                    Err(e) => error!("upgrade error: {}", e),
                }
            });

            Ok(Response::new(Body::empty()))
        } else {
            info!("CONNECT host is not socket addr: {:?}", req.uri());
            let mut resp = Response::new(Body::from("CONNECT must be to a socket address"));
            *resp.status_mut() = http::StatusCode::BAD_REQUEST;

            Ok(resp)
        }
    } else {
        client.request(req).await
    }
}

fn host_addr(uri: &http::Uri) -> Option<String> {
    uri.authority().and_then(|auth| Some(auth.to_string()))
}

// Create a TCP connection to host:port, build a tunnel between the connection and
// the upgraded connection
async fn tunnel(mut upgraded: Upgraded, addr: String) -> std::io::Result<()> {
    // Connect to remote server
    let mut server = TcpStream::connect(addr).await?;

    // Proxying data
    let (from_client, from_server) =
        tokio::io::copy_bidirectional(&mut upgraded, &mut server).await?;

    // Print message when done
    info!(
        "client wrote {} bytes and received {} bytes",
        from_client, from_server
    );

    Ok(())
}
