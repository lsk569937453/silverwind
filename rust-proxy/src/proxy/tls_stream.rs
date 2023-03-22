use core::task::{Context, Poll};
use futures_util::ready;
use hyper::server::conn::AddrStream;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::{future::Future, net::SocketAddr};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::rustls::ServerConfig;

enum State {
    Handshaking(tokio_rustls::Accept<AddrStream>),
    Streaming(tokio_rustls::server::TlsStream<AddrStream>),
}

pub struct TlsStream {
    state: State,
}

impl TlsStream {
    pub fn new(stream: AddrStream, config: Arc<ServerConfig>) -> TlsStream {
        let accept = tokio_rustls::TlsAcceptor::from(config).accept(stream);
        TlsStream {
            state: State::Handshaking(accept),
        }
    }
    pub fn remote_addr(&self) -> SocketAddr {
        match &self.state {
            State::Handshaking(accept) => {
                let addr_option = accept.get_ref();
                let socket_addr = addr_option.unwrap().remote_addr();
                return socket_addr;
            }
            State::Streaming(stream) => {
                let (addr_stream, _) = stream.get_ref();
                let socket_addr = addr_stream.remote_addr();
                return socket_addr;
            }
        }
    }
}
impl AsyncRead for TlsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        let pin = self.get_mut();
        match pin.state {
            State::Handshaking(ref mut accept) => match ready!(Pin::new(accept).poll(cx)) {
                Ok(mut stream) => {
                    let result = Pin::new(&mut stream).poll_read(cx, buf);
                    pin.state = State::Streaming(stream);
                    result
                }
                Err(err) => Poll::Ready(Err(err)),
            },
            State::Streaming(ref mut stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for TlsStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let pin = self.get_mut();
        match pin.state {
            State::Handshaking(ref mut accept) => match ready!(Pin::new(accept).poll(cx)) {
                Ok(mut stream) => {
                    let result = Pin::new(&mut stream).poll_write(cx, buf);
                    pin.state = State::Streaming(stream);
                    result
                }
                Err(err) => Poll::Ready(Err(err)),
            },
            State::Streaming(ref mut stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.state {
            State::Handshaking(_) => Poll::Ready(Ok(())),
            State::Streaming(ref mut stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.state {
            State::Handshaking(_) => Poll::Ready(Ok(())),
            State::Streaming(ref mut stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::tls_acceptor::TlsAcceptor;
    use hyper::server::conn::AddrIncoming;
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
            let tls_acceptor =
                TlsAcceptor::new(tls_cfg, AddrIncoming::from_listener(listener).unwrap());

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
