use crate::proxy::http1::tls_stream::TlsStream;
use core::task::{Context, Poll};
use futures_util::ready;
use hyper::server::accept::Accept;
use hyper::server::conn::AddrIncoming;
use std::io;
use std::pin::Pin;
use std::sync::Arc;

use tokio_rustls::rustls::ServerConfig;

pub struct TlsAcceptor {
    config: Arc<ServerConfig>,
    incoming: AddrIncoming,
}

impl TlsAcceptor {
    pub fn new(config: Arc<ServerConfig>, incoming: AddrIncoming) -> TlsAcceptor {
        TlsAcceptor { config, incoming }
    }
}

impl Accept for TlsAcceptor {
    type Conn = TlsStream;
    type Error = io::Error;

    fn poll_accept(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Conn, Self::Error>>> {
        let pin = self.get_mut();
        match ready!(Pin::new(&mut pin.incoming).poll_accept(cx)) {
            Some(Ok(sock)) => Poll::Ready(Some(Ok(TlsStream::new(sock, pin.config.clone())))),
            Some(Err(e)) => Poll::Ready(Some(Err(e))),
            None => Poll::Ready(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::http1::tls_acceptor::TlsAcceptor;
    use hyper::service::{make_service_fn, service_fn};
    use hyper::{Body, Response, Server};
    use lazy_static::lazy_static;
    use std::env;
    use std::io::BufReader;
    use std::net::SocketAddr;
    use tokio::net::TcpListener;
    use tokio::runtime::{Builder, Runtime};
    lazy_static! {
        pub static ref TOKIO_RUNTIME: Runtime = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("my-custom-name")
            .thread_stack_size(3 * 1024 * 1024)
            .max_blocking_threads(1000)
            .enable_all()
            .build()
            .unwrap();
    }
    #[test]
    fn test_tls_acceptor() {
        TOKIO_RUNTIME.spawn(async {
            let private_key_path = env::current_dir()
                .unwrap()
                .join("config")
                .join("privkey.pem");
            let private_key = std::fs::read_to_string(private_key_path).unwrap();

            let ca_certificate_path = env::current_dir()
                .unwrap()
                .join("config")
                .join("privkey.pem");
            let ca_certificate = std::fs::read_to_string(ca_certificate_path).unwrap();

            let mut cer_reader = BufReader::new(ca_certificate.as_bytes());
            let certs = rustls_pemfile::certs(&mut cer_reader)
                .unwrap()
                .iter()
                .map(|s| rustls::Certificate((*s).clone()))
                .collect();

            let doc = pkcs8::PrivateKeyDocument::from_pem(&private_key).unwrap();
            let key_der = rustls::PrivateKey(doc.as_ref().to_owned());
            let tls_cfg = {
                let cfg = rustls::ServerConfig::builder()
                    .with_safe_defaults()
                    .with_no_client_auth()
                    .with_single_cert(certs, key_der)
                    .unwrap();
                Arc::new(cfg)
            };
            let addr = SocketAddr::from(([127, 0, 0, 1], 7000));
            let listener = TcpListener::bind(&addr).await.unwrap();
            let tls_acceptor = TlsAcceptor {
                config: tls_cfg,
                incoming: AddrIncoming::from_listener(listener).unwrap(),
            };

            let make_svc = make_service_fn(|_| async {
                Ok::<_, hyper::Error>(service_fn(|_req| async {
                    Ok::<_, hyper::Error>(Response::new(Body::from("Hello World")))
                }))
            });
            let server = Server::builder(tls_acceptor).serve(make_svc);
            let _result = server.await;
        });
    }
}
