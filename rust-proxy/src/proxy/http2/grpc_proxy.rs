use crate::constants::common_constants::GRPC_STATUS_HEADER;
use crate::constants::common_constants::GRPC_STATUS_OK;
use crate::proxy::proxy_trait::CheckTrait;
use crate::proxy::proxy_trait::CommonCheckRequest;
use h2::client;
use h2::server;
use h2::server::SendResponse;
use h2::RecvStream;
use h2::SendStream;
use http::version::Version;
use http::Response;
use http::{Method, Request};
use hyper::body::Bytes;

use std::io::BufReader;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::rustls::OwnedTrustAnchor;
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::{rustls, TlsAcceptor};
use url::Url;
pub struct GrpcProxy {
    pub port: i32,
    pub channel: mpsc::Receiver<()>,
    pub mapping_key: String,
}
pub async fn start_task(
    tcp_stream: TcpStream,
    mapping_key: String,
    peer_addr: SocketAddr,
) -> Result<(), anyhow::Error> {
    let mut connection = server::handshake(tcp_stream).await?;
    while let Some(request_result) = connection.accept().await {
        if let Ok((request, respond)) = request_result {
            let mapping_key_cloned = mapping_key.clone();
            tokio::spawn(async move {
                let result =
                    request_outbound_adapter(request, respond, mapping_key_cloned, peer_addr).await;
                if result.is_err() {
                    error!(
                        "Grpc request outbound error,the error is {}",
                        result.unwrap_err()
                    );
                }
            });
        }
    }
    Ok(())
}
pub async fn start_tls_task(
    tcp_stream: TlsStream<TcpStream>,
    mapping_key: String,
    peer_addr: SocketAddr,
) -> Result<(), anyhow::Error> {
    let mut connection = server::handshake(tcp_stream).await?;
    while let Some(request_result) = connection.accept().await {
        if let Ok((request, respond)) = request_result {
            let mapping_key_cloned = mapping_key.clone();
            tokio::spawn(async move {
                let result =
                    request_outbound_adapter(request, respond, mapping_key_cloned, peer_addr).await;
                if result.is_err() {
                    error!(
                        "Grpc request outbound error,the error is {}",
                        result.unwrap_err()
                    );
                }
            });
        }
    }
    Ok(())
}
async fn request_outbound_adapter(
    inbount_request: Request<RecvStream>,
    inbound_respond: SendResponse<Bytes>,
    mapping_key: String,
    peer_addr: SocketAddr,
) -> Result<(), anyhow::Error> {
    request_outbound(
        inbount_request,
        inbound_respond,
        mapping_key,
        peer_addr,
        CommonCheckRequest {},
    )
    .await
}
impl GrpcProxy {
    pub async fn start_proxy(&mut self) -> Result<(), anyhow::Error> {
        let port_clone = self.port;
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        info!("Listening on grpc://{}", addr);
        let listener = TcpListener::bind(addr).await.unwrap();
        let mapping_key = self.mapping_key.clone();
        let reveiver = &mut self.channel;

        loop {
            let accept_future = listener.accept();
            tokio::select! {
               accept_result=accept_future=>{
                if let Ok((socket, peer_addr))=accept_result{
                    tokio::spawn(start_task(socket, mapping_key.clone(), peer_addr));
                }
               },
               _=reveiver.recv()=>{
                info!("close the socket!");
                return Ok(());
               }
            };
        }
    }
    pub async fn start_tls_proxy(
        &mut self,
        pem_str: String,
        key_str: String,
    ) -> Result<(), anyhow::Error> {
        let port_clone = self.port;
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        let mut cer_reader = BufReader::new(pem_str.as_bytes());
        let certs = rustls_pemfile::certs(&mut cer_reader)
            .unwrap()
            .iter()
            .map(|s| rustls::Certificate((*s).clone()))
            .collect();

        let doc = pkcs8::PrivateKeyDocument::from_pem(&key_str).unwrap();
        let key_der = rustls::PrivateKey(doc.as_ref().to_owned());

        let tls_cfg = {
            let cfg = rustls::ServerConfig::builder()
                .with_safe_defaults()
                .with_no_client_auth()
                .with_single_cert(certs, key_der)
                .unwrap();
            Arc::new(cfg)
        };
        let tls_acceptor = TlsAcceptor::from(tls_cfg);

        info!("Listening on grpc with tls://{}", addr);
        let listener = TcpListener::bind(addr).await.unwrap();
        let mapping_key = self.mapping_key.clone();
        let reveiver = &mut self.channel;

        loop {
            let accept_future = listener.accept();
            tokio::select! {
               accept_result=accept_future=>{
                if let Ok((tcp_stream, peer_addr))=accept_result{
                    if let Ok(tls_streams) = tls_acceptor.accept(tcp_stream).await {
                        tokio::spawn(start_tls_task(tls_streams, mapping_key.clone(), peer_addr));
                    }
                }
               },
               _=reveiver.recv()=>{
                info!("close the socket!");
                return Ok(());
               }
            };
        }
    }
}

async fn copy_io(
    mut send_stream: SendStream<Bytes>,
    mut recv_stream: RecvStream,
) -> Result<(), anyhow::Error> {
    let mut flow_control = recv_stream.flow_control().clone();
    while let Some(chunk_result) = recv_stream.data().await {
        let chunk_bytes = chunk_result?;
        debug!("Data from outbound: {:?}", chunk_bytes.clone());
        send_stream.send_data(chunk_bytes.clone(), false)?;
        flow_control.release_capacity(chunk_bytes.len())?;
    }
    if let Ok(Some(header)) = recv_stream.trailers().await {
        send_stream.send_trailers(header)?;
    }
    Ok(())
}
async fn request_outbound(
    inbount_request: Request<RecvStream>,
    mut inbound_respond: SendResponse<Bytes>,
    mapping_key: String,
    peer_addr: SocketAddr,
    check_trait: impl CheckTrait,
) -> Result<(), anyhow::Error> {
    debug!("{:?}", inbount_request);
    let (inbound_parts, inbound_body) = inbount_request.into_parts();

    let inbound_headers = inbound_parts.headers.clone();
    let uri = inbound_parts.uri.clone();
    let check_result = check_trait
        .check_before_request(mapping_key.clone(), inbound_headers, uri, peer_addr)
        .await?;
    if check_result.is_none() {
        return Err(anyhow!("The request has been denied by the proxy!"));
    }
    let request_path = check_result.unwrap().request_path;
    let url = Url::parse(&request_path)?;
    let cloned_url = url.clone();
    let host = cloned_url.host().ok_or(anyhow!("Parse host error!"))?;
    let port = cloned_url.port().ok_or(anyhow!("Parse host error!"))?;
    debug!("The host is {}", host);

    let addr = format!("{}:{}", host, port)
        .to_socket_addrs()?
        .next()
        .ok_or(anyhow!("Parse the domain error!"))?;
    debug!("The addr is {}", addr);

    let send_request_poll = if request_path.clone().contains("https") {
        let mut root_cert_store = rustls::RootCertStore::empty();
        root_cert_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(
            |ta| {
                OwnedTrustAnchor::from_subject_spki_name_constraints(
                    ta.subject,
                    ta.spki,
                    ta.name_constraints,
                )
            },
        ));
        let mut config = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_cert_store)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let tls_connector = TlsConnector::from(Arc::new(config));
        let stream = TcpStream::connect(&addr).await?;
        let domain = rustls::ServerName::try_from(host.to_string().as_str())?;
        debug!("The domain name is {}", host);
        let stream = tls_connector.connect(domain, stream).await?;
        let (send_request, connection) = client::handshake(stream).await?;
        tokio::spawn(async move {
            let connection_result = connection.await;
            if let Err(err) = connection_result {
                error!("Cause error in grpc https connection,the error is {}.", err);
            } else {
                debug!("The connection has closed!");
            }
        });
        send_request
    } else {
        let tcpstream = TcpStream::connect(addr).await?;
        let (send_request, connection) = client::handshake(tcpstream).await?;
        tokio::spawn(async move {
            connection.await.unwrap();
            debug!("The connection has closed!");
        });
        send_request
    };

    debug!("request path is {}", url.to_string());
    let mut send_request = send_request_poll.ready().await?;
    let request = Request::builder()
        .method(Method::POST)
        .version(Version::HTTP_2)
        .uri(url.to_string())
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .body(())
        .unwrap();
    debug!("Our bound request is {:?}", request);
    let (response, outbound_send_stream) = send_request.send_request(request, false)?;
    tokio::spawn(async {
        if let Err(err) = copy_io(outbound_send_stream, inbound_body).await {
            error!("Copy from inbound to outboud error,the error is {}", err);
        }
    });

    let (head, outboud_response_body) = response
        .await
        .map_err(|e| anyhow!(e.to_string()))?
        .into_parts();

    debug!("Received response: {:?}", head);

    let header_map = head.headers.clone();
    let is_grpc_status_ok = header_map
        .get(GRPC_STATUS_HEADER)
        .map(|item| item.to_str().unwrap_or_default() != GRPC_STATUS_OK)
        .unwrap_or(false);
    let inbound_response = Response::from_parts(head, ());

    let send_stream = inbound_respond
        .send_response(inbound_response, is_grpc_status_ok)
        .map_err(|e| anyhow!(e.to_string()))?;

    tokio::spawn(async {
        if let Err(err) = copy_io(send_stream, outboud_response_body).await {
            error!("Copy from outbound to inbound error,the error is {}", err);
        }
    });
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::http1::http_client::HttpClients;
    use crate::proxy::proxy_trait::CheckResult;
    use crate::vojo::app_config::Route;
    use async_trait::async_trait;
    use hyper::HeaderMap;
    use hyper::Uri;

    use hyper::Body;
    use hyper::StatusCode;
    use lazy_static::lazy_static;
    use std::env;
    use std::time::Duration;
    use tokio::runtime::{Builder, Runtime};
    use tokio::time::sleep;
    struct MockProvider();
    #[async_trait]
    impl CheckTrait for MockProvider {
        async fn check_before_request(
            &self,
            _mapping_key: String,
            _headers: HeaderMap,
            _uri: Uri,
            _peer_addr: SocketAddr,
        ) -> Result<Option<CheckResult>, anyhow::Error> {
            let route = Route::from(Default::default()).await?;
            Ok(Some(CheckResult {
                request_path: String::from("http://127.0.0.1:50051"),
                route,
                base_route: Default::default(),
            }))
        }
    }

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

    #[tokio::test]
    async fn test_grpc_ok() {
        tokio::spawn(async {
            let (_, receiver) = tokio::sync::mpsc::channel(10);

            let mut http_proxy = GrpcProxy {
                port: 3257,
                channel: receiver,
                mapping_key: String::from("random key"),
            };
            let _result = http_proxy.start_proxy().await;
        });
        sleep(Duration::from_millis(100)).await;

        let request = Request::builder()
            .method(Method::POST)
            .version(Version::HTTP_2)
            .uri("http://127.0.0.1:3527")
            .header("content-type", "application/grpc")
            .header("te", "trailers")
            .body(Body::empty())
            .unwrap();
        let http_clients = HttpClients::new();
        let outbound_res = http_clients.request_http(request, 3).await;

        if let Ok(Ok(response)) = outbound_res {
            assert_eq!(response.status(), StatusCode::OK);
        }
    }
    #[tokio::test]
    async fn test_grpc_tls_ok() {
        let private_key_path = env::current_dir()
            .unwrap()
            .join("config")
            .join("test_key.pem");
        let private_key = std::fs::read_to_string(private_key_path).unwrap();

        let ca_certificate_path = env::current_dir()
            .unwrap()
            .join("config")
            .join("test_key.pem");
        let ca_certificate = std::fs::read_to_string(ca_certificate_path).unwrap();

        tokio::spawn(async {
            let (_, receiver) = tokio::sync::mpsc::channel(10);

            let mut http_proxy = GrpcProxy {
                port: 5746,
                channel: receiver,
                mapping_key: String::from("random key"),
            };
            let _result = http_proxy
                .start_tls_proxy(ca_certificate, private_key)
                .await;
        });
        sleep(Duration::from_millis(100)).await;

        let request = Request::builder()
            .method(Method::POST)
            .version(Version::HTTP_2)
            .uri("https://127.0.0.1:5746")
            .header("content-type", "application/grpc")
            .header("te", "trailers")
            .body(Body::empty())
            .unwrap();
        let http_clients = HttpClients::new();
        let outbound_res = http_clients.request_http(request, 3).await;
        if let Ok(Ok(response)) = outbound_res {
            assert_eq!(response.status(), StatusCode::OK);
        }
    }
}
