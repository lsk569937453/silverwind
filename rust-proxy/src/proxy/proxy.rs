use http::StatusCode;
use hyper::client::HttpConnector;
use hyper::upgrade::Upgraded;
use hyper::{Body, Client, Request, Response, Server};
// use pool::MyError;
use crate::configuration_service::app_config_servive::GLOBAL_CONFIG_MAPPING;
use hyper::service::{make_service_fn, service_fn};
use hyper_tls::HttpsConnector;
use regex::Regex;
use std::convert::Infallible;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct HttpProxy {
    pub port: i32,
    pub channel: mpsc::Receiver<()>,
}
#[derive(Clone)]
pub struct Clients {
    pub http_client: Client<HttpConnector>,
    pub https_client: hyper::Client<hyper_tls::HttpsConnector<HttpConnector>>,
}
impl Clients {
    fn new() -> Clients {
        let http_client = Client::builder()
            .http1_title_case_headers(true)
            .http1_preserve_header_case(true)
            .build_http();
        let https = HttpsConnector::new();
        let https_client = Client::builder().build::<_, hyper::Body>(https);
        return Clients {
            http_client: http_client,
            https_client: https_client,
        };
    }
    async fn request_http(&self, req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        return self.http_client.request(req).await;
    }
    async fn request_https(&self, req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        return self.https_client.request(req).await;
    }
}

impl HttpProxy {
    pub async fn start(&mut self) {
        let port_clone = self.port.clone();
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        let client = Clients::new();
        let make_service = make_service_fn(move |_| {
            let client = client.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    proxy(port_clone.clone(), client.clone(), req)
                }))
            }
        });
        let server = Server::bind(&addr)
            .http1_preserve_header_case(true)
            .http1_title_case_headers(true)
            .serve(make_service);
        info!("Listening on http://{}", addr);

        let reveiver = &mut self.channel;

        let graceful = server.with_graceful_shutdown(async move {
            reveiver.recv().await;
        });

        // Await the `server` receiving the signal...
        if let Err(e) = graceful.await {
            info!("server has receive error: {}", e);
        }
    }
}
async fn proxy(
    port: i32,
    client: Clients,
    mut req: Request<Body>,
) -> Result<Response<Body>, hyper::Error> {
    debug!("req: {:?}", req);

    let backend_path = req.uri().path();
    let config = GLOBAL_CONFIG_MAPPING.get(&port).unwrap().clone();

    for item in config.routes {
        let match_prefix = item.matcher.prefix;
        let re = Regex::new(match_prefix.as_str()).unwrap();
        let match_res = re.captures(backend_path);
        if match_res.is_some() {
            let request_path = format!("{}{}", item.route_cluster, match_prefix.clone());
            *req.uri_mut() = request_path.parse().unwrap();
            if request_path.contains("https") {
                return client.request_https(req).await;
            } else {
                return client.request_http(req).await;
            }
        }
    }
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from(
            r#"{
            "response_code": -1,
            "response_object": "the path could not find in the Proxy!"
        }"#,
        ))
        .unwrap())
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
mod tests {
    use super::*;

    #[test]
    fn test_output_serde() {
        let re = Regex::new("/v1/proxy").unwrap();
        let caps1 = re.captures("/v1/proxy");
        let caps2 = re.captures("/v1/proxy/api");
        let caps3 = re.captures("/v1/proxy/api?test=1");
        let caps4 = re.captures("/v1/prox");
        assert_eq!(caps1.is_some(), true);
        assert_eq!(caps2.is_some(), true);
        assert_eq!(caps3.is_some(), true);
        assert_eq!(caps4.is_some(), false);
    }
}
