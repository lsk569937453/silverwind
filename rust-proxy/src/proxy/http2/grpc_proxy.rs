use crate::constants::common_constants::GRPC_STATUS_HEADER;
use crate::constants::common_constants::GRPC_STATUS_OK;
use crate::proxy::proxy_trait::CheckTrait;
use crate::proxy::proxy_trait::CommonCheckRequest;
use crate::vojo::app_error::AppError;
use h2::client;
use h2::server;
use h2::server::SendResponse;
use h2::RecvStream;
use h2::SendStream;
use http::version::Version;
use http::Response;
use http::{Method, Request};
use hyper::body::Bytes;

use rustls_pki_types::CertificateDer;
use std::io::BufReader;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
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
) -> Result<(), AppError> {
    let mut connection = server::handshake(tcp_stream)
        .await
        .map_err(|e| AppError(e.to_string()))?;
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
) -> Result<(), AppError> {
    let mut connection = server::handshake(tcp_stream)
        .await
        .map_err(|e| AppError(e.to_string()))?;
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
) -> Result<(), AppError> {
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
    pub async fn start_proxy(&mut self) -> Result<(), AppError> {
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
                info!("close the socket of grpc!");
                return Ok(());
               }
            };
        }
    }
    pub async fn start_tls_proxy(
        &mut self,
        pem_str: String,
        key_str: String,
    ) -> Result<(), AppError> {
        let port_clone = self.port;
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        let mut cer_reader = BufReader::new(pem_str.as_bytes());
        // let certs = rustls_pemfile::certs(&mut cer_reader)
        //     .unwrap()
        //     .iter()
        //     .map(|s| rustls::Certificate((*s).clone()))
        //     .collect();
        let certs: Vec<CertificateDer<'_>> = rustls_pemfile::certs(&mut cer_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError(e.to_string()))?;

        let mut key_reader = BufReader::new(key_str.as_bytes());
        let key_der = rustls_pemfile::private_key(&mut key_reader)
            .map(|key| key.unwrap())
            .map_err(|e| AppError(e.to_string()))?;

        let tls_cfg = {
            let cfg = rustls::ServerConfig::builder()
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
) -> Result<(), AppError> {
    let mut flow_control = recv_stream.flow_control().clone();
    while let Some(chunk_result) = recv_stream.data().await {
        let chunk_bytes = chunk_result.map_err(|e| AppError(e.to_string()))?;
        debug!("Data from outbound: {:?}", chunk_bytes.clone());
        send_stream
            .send_data(chunk_bytes.clone(), false)
            .map_err(|e| AppError(e.to_string()))?;
        flow_control
            .release_capacity(chunk_bytes.len())
            .map_err(|e| AppError(e.to_string()))?;
    }
    if let Ok(Some(header)) = recv_stream.trailers().await {
        send_stream
            .send_trailers(header)
            .map_err(|e| AppError(e.to_string()))?;
    }
    Ok(())
}
async fn request_outbound(
    inbount_request: Request<RecvStream>,
    mut inbound_respond: SendResponse<Bytes>,
    mapping_key: String,
    peer_addr: SocketAddr,
    check_trait: impl CheckTrait,
) -> Result<(), AppError> {
    debug!("{:?}", inbount_request);
    let (inbound_parts, inbound_body) = inbount_request.into_parts();

    let inbound_headers = inbound_parts.headers.clone();
    let uri = inbound_parts.uri.clone();
    let check_result = check_trait
        .check_before_request(
            Arc::new(Mutex::new(Default::default())),
            mapping_key.clone(),
            inbound_headers,
            uri,
            peer_addr,
        )
        .await?;
    if check_result.is_none() {
        return Err(AppError(String::from(
            "The request has been denied by the proxy!",
        )));
    }
    let request_path = check_result.unwrap().request_path;
    let url = Url::parse(&request_path).map_err(|e| AppError(e.to_string()))?;
    let cloned_url = url.clone();
    let host = cloned_url
        .host()
        .ok_or(AppError(String::from("Parse host error!")))?;
    let port = cloned_url
        .port()
        .ok_or(AppError(String::from("Parse host error!")))?;
    debug!("The host is {}", host);

    let addr = format!("{}:{}", host, port)
        .to_socket_addrs()
        .map_err(|e| AppError(e.to_string()))?
        .next()
        .ok_or(AppError(String::from("Parse the domain error!")))?;
    debug!("The addr is {}", addr);
    let host_str = host.to_string();

    let send_request_poll = if request_path.clone().contains("https") {
        let mut root_cert_store = rustls::RootCertStore::empty();
        root_cert_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let mut config = rustls::ClientConfig::builder()
            .with_root_certificates(root_cert_store)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let tls_connector = TlsConnector::from(Arc::new(config));
        let stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| AppError(e.to_string()))?;
        let domain = rustls_pki_types::ServerName::try_from(host_str.as_str())
            .map_err(|e| AppError(e.to_string()))?
            .to_owned();
        debug!("The domain name is {}", host);
        let stream = tls_connector
            .connect(domain, stream)
            .await
            .map_err(|e| AppError(e.to_string()))?;
        let (send_request, connection) = client::handshake(stream)
            .await
            .map_err(|e| AppError(e.to_string()))?;
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
        let tcpstream = TcpStream::connect(addr)
            .await
            .map_err(|e| AppError(e.to_string()))?;
        let (send_request, connection) = client::handshake(tcpstream)
            .await
            .map_err(|e| AppError(e.to_string()))?;
        tokio::spawn(async move {
            connection.await.unwrap();
            debug!("The connection has closed!");
        });
        send_request
    };

    debug!("request path is {}", url.to_string());
    let mut send_request = send_request_poll
        .ready()
        .await
        .map_err(|e| AppError(e.to_string()))?;
    let request = Request::builder()
        .method(Method::POST)
        .version(Version::HTTP_2)
        .uri(url.to_string())
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .body(())
        .unwrap();
    debug!("Our bound request is {:?}", request);
    let (response, outbound_send_stream) = send_request
        .send_request(request, false)
        .map_err(|e| AppError(e.to_string()))?;
    tokio::spawn(async {
        if let Err(err) = copy_io(outbound_send_stream, inbound_body).await {
            error!("Copy from inbound to outboud error,the error is {}", err);
        }
    });

    let (head, outboud_response_body) = response
        .await
        .map_err(|e| AppError(e.to_string()))?
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
        .map_err(|e| AppError(e.to_string()))?;

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
    use crate::vojo::app_config::AppConfig;
    use crate::vojo::app_config::LivenessConfig;
    use crate::vojo::app_config::LivenessStatus;
    use crate::vojo::app_config::Matcher;
    use crate::vojo::app_config::Route;
    use crate::vojo::health_check::BaseHealthCheckParam;
    use crate::vojo::health_check::HealthCheckType;
    use crate::vojo::health_check::HttpHealthCheckParam;
    use crate::vojo::route::AnomalyDetectionStatus;
    use crate::vojo::route::BaseRoute;
    use crate::vojo::route::LoadbalancerStrategy;
    use crate::vojo::route::WeightRoute;
    use crate::vojo::route::WeightRouteNestedItem;
    use async_trait::async_trait;
    use http_body_util::BodyExt;
    use http_body_util::Full;
    use hyper::HeaderMap;
    use hyper::StatusCode;
    use hyper::Uri;
    use std::env;
    use std::time::Duration;
    use tokio::runtime::{Builder, Runtime};
    use tokio::time::sleep;
    use uuid::Uuid;
    fn create_route() -> Route {
        let id = Uuid::new_v4();
        let route = Route {
            host_name: None,
            route_id: id.to_string(),
            route_cluster: LoadbalancerStrategy::WeightRoute(WeightRoute {
                index: 0,
                offset: 0,
                routes: vec![WeightRouteNestedItem {
                    base_route: BaseRoute {
                        endpoint: String::from("http://www.937453.xyz"),
                        try_file: None,
                        base_route_id: String::from(""),
                        is_alive: None,
                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    },
                    weight: 100,
                }],
            }),
            liveness_status: LivenessStatus {
                current_liveness_count: 0,
            },
            anomaly_detection: None,
            health_check: Some(HealthCheckType::HttpGet(HttpHealthCheckParam {
                base_health_check_param: BaseHealthCheckParam {
                    timeout: 0,
                    interval: 2,
                },
                path: String::from("/"),
            })),
            liveness_config: Some(LivenessConfig {
                min_liveness_count: 1,
            }),
            allow_deny_list: None,
            rewrite_headers: None,

            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("/"),
                prefix_rewrite: String::from("/"),
            }),
        };
        route
    }
    struct MockProvider();
    #[async_trait]
    impl CheckTrait for MockProvider {
        async fn check_before_request(
            &self,
            shared_config: Arc<Mutex<AppConfig>>,
            _mapping_key: String,
            _headers: HeaderMap,
            _uri: Uri,
            _peer_addr: SocketAddr,
        ) -> Result<Option<CheckResult>, AppError> {
            let route = create_route();
            Ok(Some(CheckResult {
                request_path: String::from("http://127.0.0.1:50051"),
                route,
                base_route: Default::default(),
            }))
        }
    }

    #[tokio::test]
    async fn test_grpc_ok() {
        let (sender, receiver) = tokio::sync::mpsc::channel(10);

        tokio::spawn(async {
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
            .body(Full::new(Bytes::new()).boxed())
            .unwrap();
        let http_clients = HttpClients::new(false);
        let outbound_res = http_clients.request_http(request, 3).await;

        if let Ok(Ok(response)) = outbound_res {
            assert_eq!(response.status(), StatusCode::OK);
        }
        sender.send(()).await;
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
        let (sender, receiver) = tokio::sync::mpsc::channel(10);

        tokio::spawn(async {
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
            .body(Full::new(Bytes::new()).boxed())
            .unwrap();
        let http_clients = HttpClients::new(false);
        let outbound_res = http_clients.request_http(request, 3).await;
        if let Ok(Ok(response)) = outbound_res {
            assert_eq!(response.status(), StatusCode::OK);
        }
        sender.send(()).await;
    }
}
