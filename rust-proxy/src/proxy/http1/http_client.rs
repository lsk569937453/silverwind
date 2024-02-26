use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::Body;
use hyper::body::Incoming;
use hyper::Request;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::ResponseFuture;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use rustls::RootCertStore;
use std::convert::Infallible;
use std::time::Duration;
use tokio::time::timeout;
use tokio::time::Timeout;

#[derive(Clone)]
pub struct HttpClients {
    pub http_client: Client<HttpConnector, BoxBody<Bytes, Infallible>>,
    pub https_client:
        Client<hyper_rustls::HttpsConnector<HttpConnector>, BoxBody<Bytes, Infallible>>,
}
impl HttpClients {
    pub fn new() -> HttpClients {
        let http_client = Client::builder(TokioExecutor::new())
            .http1_title_case_headers(true)
            .http1_preserve_header_case(true)
            .build_http();
        let mut root_store = RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_or_http()
            .enable_http1()
            .build();
        let https_client = Client::builder(TokioExecutor::new()).build(https);
        HttpClients {
            http_client,
            https_client,
        }
    }
    pub fn request_http(
        &self,
        req: Request<BoxBody<Bytes, Infallible>>,
        time_out: u64,
    ) -> Timeout<ResponseFuture> {
        let request_future = self.http_client.request(req);
        timeout(Duration::from_secs(time_out), request_future)
    }
    pub fn request_https(
        &self,
        req: Request<BoxBody<Bytes, Infallible>>,
        time_out: u64,
    ) -> Timeout<ResponseFuture> {
        let request_future = self.https_client.request(req);
        timeout(Duration::from_secs(time_out), request_future)
    }
}
