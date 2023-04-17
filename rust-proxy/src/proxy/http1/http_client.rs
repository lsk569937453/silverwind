use hyper::client::HttpConnector;
use hyper::client::ResponseFuture;
use hyper::Body;
use hyper::Client;
use hyper::Request;
use hyper_rustls::ConfigBuilderExt;
use rustls::{OwnedTrustAnchor, RootCertStore};
use std::time::Duration;
use tokio::time::timeout;
use tokio::time::Timeout;
#[derive(Clone)]
pub struct HttpClients {
    pub http_client: Client<HttpConnector>,
    pub https_client: Client<hyper_rustls::HttpsConnector<HttpConnector>>,
}
impl HttpClients {
    pub fn new() -> HttpClients {
        let http_client = Client::builder()
            .http1_title_case_headers(true)
            .http1_preserve_header_case(true)
            .build_http();
        let mut root_store = RootCertStore::empty();
        root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
            OwnedTrustAnchor::from_subject_spki_name_constraints(
                ta.subject,
                ta.spki,
                ta.name_constraints,
            )
        }));
        let tls = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_webpki_roots()
            .with_no_client_auth();
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_or_http()
            .enable_http1()
            .build();
        let https_client = Client::builder().build::<_, hyper::Body>(https);
        HttpClients {
            http_client,
            https_client,
        }
    }
    pub fn request_http(&self, req: Request<Body>, time_out: u64) -> Timeout<ResponseFuture> {
        let request_future = self.http_client.request(req);
        timeout(Duration::from_secs(time_out), request_future)
    }
    pub fn request_https(&self, req: Request<Body>, time_out: u64) -> Timeout<ResponseFuture> {
        let request_future = self.https_client.request(req);
        timeout(Duration::from_secs(time_out), request_future)
    }
}
