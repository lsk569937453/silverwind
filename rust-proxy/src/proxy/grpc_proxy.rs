use futures::FutureExt;

use crate::configuration_service::app_config_service::GLOBAL_CONFIG_MAPPING;
use futures::Future;
use h2::client;
use h2::server;
use h2::server::SendResponse;
use h2::RecvStream;
use h2::SendStream;
use http::version::Version;
use http::{Method, Request};
use http::{Response, StatusCode};
use hyper::body::Bytes;
use hyper::service::{make_service_fn, service_fn};
use hyper::HeaderMap;
use hyper::Uri;
use hyper::{Client, Error, Server};
use std::convert::TryFrom;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::rustls::{OwnedTrustAnchor, RootCertStore, ServerName};
use tokio_rustls::TlsConnector;
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
    let mut h2 = server::handshake(tcp_stream).await?;
    while let Some(request_result) = h2.accept().await {
        if let Ok((request, respond)) = request_result {
            let result =
                request_outbound_adapter(request, respond, mapping_key.clone(), peer_addr).await;
            if result.is_err() {
                error!(
                    "Grpc request outbound error,the error is {}",
                    result.unwrap_err()
                );
            }
        }
    }
    Ok(())
}
async fn request_outbound_adapter(
    mut inbount_request: Request<RecvStream>,
    mut inbound_respond: SendResponse<Bytes>,
    mapping_key: String,
    peer_addr: SocketAddr,
) -> Result<(), anyhow::Error> {
    request_outbound(inbount_request, inbound_respond, mapping_key, peer_addr).await
}
impl GrpcProxy {
    pub async fn start_proxy(&mut self) -> Result<(), anyhow::Error> {
        let port_clone = self.port;
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        info!("Listening on grpc://{}", addr);

        let listener = TcpListener::bind(addr).await.unwrap();
        let mapping_key = self.mapping_key.clone();

        loop {
            if let Ok((socket, peer_addr)) = listener.accept().await {
                tokio::spawn(start_task(socket, mapping_key.clone(), peer_addr.clone()));
            }
        }
    }
}

pub async fn check_before_request(
    mapping_key: String,
    headers: HeaderMap,
    uri: Uri,
    peer_addr: SocketAddr,
) -> Result<Option<String>, anyhow::Error> {
    let backend_path = uri.path();
    let api_service_manager = GLOBAL_CONFIG_MAPPING
        .get(&mapping_key)
        .ok_or(anyhow!(format!(
            "Can not find the config mapping on the key {}!",
            mapping_key.clone()
        )))?
        .clone();
    let addr_string = peer_addr.ip().to_string();
    for item in api_service_manager.service_config.routes {
        let match_result = item.is_matched(backend_path, Some(headers.clone()))?;
        if match_result.clone().is_none() {
            continue;
        }
        let is_allowed = item
            .is_allowed(addr_string.clone(), Some(headers.clone()))
            .await?;
        if !is_allowed {
            return Ok(None);
        }
        let base_route = item
            .route_cluster
            .clone()
            .get_route(headers.clone())
            .await?;
        let endpoint = base_route.clone().endpoint;
        let host = Url::parse(endpoint.as_str())?;
        let rest_path = match_result.unwrap();

        let request_path = host.join(rest_path.clone().as_str())?.to_string();
        return Ok(Some(request_path));
    }
    Ok(None)
}
async fn request_outbound(
    mut inbount_request: Request<RecvStream>,
    mut inbound_respond: SendResponse<Bytes>,
    mapping_key: String,
    peer_addr: SocketAddr,
) -> Result<(), anyhow::Error> {
    info!("{:?}", inbount_request);
    let request_headers = inbount_request.headers().clone();
    let uri = inbount_request.uri().clone();
    let check_result =
        check_before_request(mapping_key.clone(), request_headers, uri, peer_addr).await?;
    if check_result.is_none() {
        return Err(anyhow!("The request has been denied by the proxy!"));
    }
    let request_path = check_result.unwrap();
    let url = Url::parse(&request_path)?;
    let cloned_url = url.clone();
    let host = cloned_url.host().ok_or(anyhow!("Parse host error!"))?;
    let port = cloned_url.port().ok_or(anyhow!("Parse host error!"))?;
    let tcp = TcpStream::connect(format!("{}:{}", host, port)).await?;
    let (h2, connection) = client::handshake(tcp).await?;
    tokio::spawn(async move {
        connection.await.unwrap();
    });
    info!("request path is {}", request_path);

    let mut h2 = h2.ready().await?;
    let request = Request::builder()
        .method(Method::POST)
        .uri(request_path)
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .body(())
        .unwrap();

    let (response, mut outbound_send_stream) = h2.send_request(request, false)?;
    let body = inbount_request.body_mut();

    let mut count = 0;
    while let Some(data) = body.data().await {
        let data = data.map_err(|e| anyhow!(e.to_string()))?;
        println!("<<<< recv {:?}", data);
        let _ = body.flow_control().release_capacity(data.len());
        outbound_send_stream.send_data(data.clone(), count == 1)?;
        count += 1;
    }

    let (head, mut outboud_response_body) = response
        .await
        .map_err(|e| anyhow!(e.to_string()))?
        .into_parts();

    println!("Received response: {:?}", head);
    let inbound_response = Response::builder()
        .header("content-type", "application/grpc")
        .status(StatusCode::OK)
        .version(Version::HTTP_2)
        .body(())
        .unwrap();

    let mut send_stream = inbound_respond
        .send_response(inbound_response, false)
        .map_err(|e| anyhow!(e.to_string()))?;

    let mut flow_control = outboud_response_body.flow_control().clone();

    while let Some(chunk) = outboud_response_body.data().await {
        let chunk_bytes = chunk?;
        println!("RX: {:?}", chunk_bytes.clone());

        let trtr = send_stream.send_data(chunk_bytes.clone(), false).unwrap();
        let _ = flow_control.release_capacity(chunk_bytes.len()).unwrap();
    }
    let mut headers = HeaderMap::new();
    headers.insert("grpc-status", "0".parse().unwrap());
    headers.insert("trace-proto-bin", "jher831yy13JHy3hc".parse().unwrap());
    send_stream.send_trailers(headers).unwrap();
    Ok(())
}
