use bytes::Bytes;
use http_body_util::combinators::BoxBody;

use hyper::Request;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::ResponseFuture;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use rustls::client::danger::HandshakeSignatureValid;
use rustls::client::danger::ServerCertVerifier;
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::ServerName;
use rustls::pki_types::{CertificateDer, TrustAnchor, UnixTime};
use rustls::RootCertStore;
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tokio::time::Timeout;
#[derive(Clone)]
pub struct HttpClients {
    pub http_client: Client<HttpConnector, BoxBody<Bytes, Infallible>>,
    pub https_client:
        Client<hyper_rustls::HttpsConnector<HttpConnector>, BoxBody<Bytes, Infallible>>,
}
#[derive(Debug)]
struct DummyTlsVerifier;

impl ServerCertVerifier for DummyTlsVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
impl HttpClients {
    pub fn new(skip_certificate_validate: bool) -> HttpClients {
        let http_client = Client::builder(TokioExecutor::new())
            .http1_title_case_headers(true)
            .http1_preserve_header_case(true)
            .build_http();
        let mut root_store = RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let mut tls = rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        if skip_certificate_validate {
            tls = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(DummyTlsVerifier {}))
                .with_no_client_auth();
        }
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
