use futures::TryFutureExt;
use tokio::io;

use crate::configuration_service::app_config_service::GLOBAL_CONFIG_MAPPING;

use base64::{engine::general_purpose, Engine as _};
use http::HeaderMap;
use hyper::header::{HeaderValue, CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, UPGRADE};
use hyper::server::conn::AddrIncoming;

use crate::proxy::tls_acceptor::TlsAcceptor;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Request, Response, Server, StatusCode};
use hyper_tls::HttpsConnector;
use sha1::{Digest, Sha1};
use std::io::BufReader;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct WebsocketProxy {
    pub port: i32,
    pub channel: mpsc::Receiver<()>,
    pub mapping_key: String,
}
async fn server_upgraded_io(
    inbound_req: Request<Body>,
    outbound_res: Response<Body>,
) -> Result<(), anyhow::Error> {
    let inbound = hyper::upgrade::on(inbound_req).await?;
    let outbound = hyper::upgrade::on(outbound_res).await?;
    let (mut ri, mut wi) = tokio::io::split(inbound);
    let (mut ro, mut wo) = tokio::io::split(outbound);
    let client_to_server = async {
        io::copy(&mut ri, &mut wo).await?;
        wo.shutdown().await
    };

    let server_to_client = async {
        io::copy(&mut ro, &mut wi).await?;
        wi.shutdown().await
    };

    let result = tokio::try_join!(client_to_server, server_to_client);

    if result.is_err() {
        error!("Copy stream error!");
    }

    Ok(())
}
async fn get_route_cluster(mapping_key: String) -> Result<String, anyhow::Error> {
    let value = GLOBAL_CONFIG_MAPPING
        .get(&mapping_key)
        .ok_or("Can not get apiservice from global_mapping")
        .map_err(|err| anyhow!(err.to_string()))?;
    let service_config = &value.service_config.routes.clone();
    let service_config_clone = service_config.clone();
    if service_config_clone.is_empty() {
        return Err(anyhow!("The len of routes is 0"));
    }
    let mut route = service_config_clone.first().unwrap().route_cluster.clone();
    route.get_route(HeaderMap::new()).await.map(|s| s.endpoint)
}
async fn server_upgrade(
    req: Request<Body>,
    mapping_key: String,
) -> Result<Response<Body>, anyhow::Error> {
    let mut res = Response::new(Body::empty());
    if !req.headers().contains_key(UPGRADE) {
        *res.status_mut() = StatusCode::BAD_REQUEST;
        return Ok(res);
    }

    let header_map = req.headers().clone();
    let upgrade_value = header_map.get(UPGRADE).unwrap();
    let sec_websocke_key = header_map
        .get(SEC_WEBSOCKET_KEY)
        .ok_or(anyhow!("Can not get the websocket key!"))?
        .to_str()?
        .to_string();

    let proxy_addr = get_route_cluster(mapping_key).await?;
    let mut new_request = Request::builder()
        .method(req.method().clone())
        .uri(proxy_addr)
        .body(Body::empty())?;

    let new_header = new_request.headers_mut();
    header_map.iter().for_each(|(key, value)| {
        new_header.insert(key, value.clone());
    });
    info!("print req:{:?}", new_request);

    let outbound_res = if req.uri().to_string().contains("https") {
        let https = HttpsConnector::new();
        let client = Client::builder().build::<_, hyper::Body>(https);
        client.request(new_request).await?
    } else {
        let client = Client::new();
        client.request(new_request).await?
    };
    if outbound_res.status() != StatusCode::SWITCHING_PROTOCOLS {
        return Err(anyhow!("Request error!"));
    }
    tokio::task::spawn(async move {
        let res = server_upgraded_io(req, outbound_res).await;
        if let Err(err) = res {
            error!("{}", err);
        }
    });
    let web_socket_value = format!("{}258EAFA5-E914-47DA-95CA-C5AB0DC85B11", sec_websocke_key);
    let mut hasher = Sha1::new();
    hasher.update(web_socket_value);
    let result = hasher.finalize();
    let encoded: String = general_purpose::STANDARD.encode(result);
    *res.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    res.headers_mut().insert(UPGRADE, upgrade_value.clone());
    res.headers_mut().insert(
        SEC_WEBSOCKET_ACCEPT,
        HeaderValue::from_str(encoded.as_str())?,
    );
    res.headers_mut()
        .insert("Connection", HeaderValue::from_str("Upgrade")?);
    Ok(res)
}
impl WebsocketProxy {
    pub async fn start_proxy(&mut self) -> Result<(), anyhow::Error> {
        let port_clone = self.port;
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        let mapping_key = self.mapping_key.clone();
        let make_service = make_service_fn(move |_| {
            let mapping_key1 = mapping_key.clone();
            async {
                Ok::<_, hyper::Error>(service_fn(move |req| {
                    server_upgrade(req, mapping_key1.clone()).map_err(|e| {
                        error!("{}", e);
                        e
                    })
                }))
            }
        });
        info!("Listening on: {}", addr);
        let server = Server::bind(&addr).serve(make_service);
        let reveiver = &mut self.channel;
        let server = server.with_graceful_shutdown(async move {
            reveiver.recv().await;
        });

        if let Err(e) = server.await {
            eprintln!("server error: {}", e);
        }
        Ok(())
    }
    pub async fn start_tls_proxy(
        &mut self,
        pem_str: String,
        key_str: String,
    ) -> Result<(), anyhow::Error> {
        let port_clone = self.port;
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        let mapping_key = self.mapping_key.clone();
        let make_service = make_service_fn(move |_| {
            let mapping_key1 = mapping_key.clone();
            async {
                Ok::<_, hyper::Error>(service_fn(move |req| {
                    server_upgrade(req, mapping_key1.clone())
                }))
            }
        });
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
        let incoming = AddrIncoming::bind(&addr).map_err(|e| {
            anyhow!(
                "Cause error when binding the socket,the addr is {},the error is {}.",
                addr.clone(),
                e.to_string()
            )
        })?;
        info!("Listening on: {}", addr);
        let server = Server::builder(TlsAcceptor::new(tls_cfg, incoming)).serve(make_service);
        let reveiver = &mut self.channel;
        let server = server.with_graceful_shutdown(async move {
            reveiver.recv().await;
        });

        if let Err(e) = server.await {
            eprintln!("server error: {}", e);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::uuid::get_uuid;
    use lazy_static::lazy_static;
    use regex::Regex;
    use std::env;
    use std::fs::File;
    use std::io::BufReader;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::runtime::{Builder, Runtime};
    use tokio::sync::RwLock;
    use tokio::time::sleep;
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
    async fn test_https_client_ok() {
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

            let mut http_proxy = WebsocketProxy {
                port: 1223,
                channel: receiver,
                mapping_key: String::from("random key"),
            };
            let _result = http_proxy
                .start_tls_proxy(ca_certificate, private_key)
                .await;
        });
        sleep(Duration::from_millis(100)).await;

        let mut req = Request::builder()
            .uri("https://localhost:1223/")
            .body(Body::empty())
            .unwrap();
        req.headers_mut().insert(
            SEC_WEBSOCKET_KEY,
            HeaderValue::from_static("fXBc01czvBdpDJEZtq7D4w=="),
        );
        req.headers_mut()
            .insert(CONNECTION, HeaderValue::from_static("Upgrade"));
        req.headers_mut()
            .insert(UPGRADE, HeaderValue::from_static("websocket"));
        let https = HttpsConnector::new();
        let client = Client::builder().build::<_, hyper::Body>(https);

        let outbound_res = client.request(req).await.unwrap_or_default();
        assert_eq!(outbound_res.status(), StatusCode::OK);
    }
    #[tokio::test]
    async fn test_http_client_ok() {
        tokio::spawn(async {
            let (_, receiver) = tokio::sync::mpsc::channel(10);

            let mut http_proxy = WebsocketProxy {
                port: 1489,
                channel: receiver,
                mapping_key: String::from("random key"),
            };
            let _result = http_proxy.start_proxy().await;
            assert!(_result.is_ok());
        });
        sleep(Duration::from_millis(100)).await;

        let mut req = Request::builder()
            .uri("http://localhost:1489/")
            .body(Body::empty())
            .unwrap();
        req.headers_mut().insert(
            SEC_WEBSOCKET_KEY,
            HeaderValue::from_static("fXBc01czvBdpDJEZtq7D4w=="),
        );
        req.headers_mut()
            .insert(CONNECTION, HeaderValue::from_static("Upgrade"));
        req.headers_mut()
            .insert(UPGRADE, HeaderValue::from_static("websocket"));
        let client = Client::new();
        let mut outbound_res = client.request(req).await.unwrap_or_default();

        assert_eq!(outbound_res.status(), StatusCode::OK)
    }
}
