use crate::constants::common_constants;
use crate::constants::common_constants::DEFAULT_HTTP_TIMEOUT;
use crate::monitor::prometheus_exporter::{get_timer_list, inc};
use crate::proxy::http1::http_client::HttpClients;

use crate::vojo::anomaly_detection::AnomalyDetectionType;
use crate::vojo::app_config::{LivenessConfig, LivenessStatus};
use crate::vojo::route::BaseRoute;
use bytes::Bytes;
use futures::FutureExt;
use http::uri::InvalidUri;
use http::Uri;
use hyper::body::Incoming;
use hyper::header::{CONNECTION, SEC_WEBSOCKET_KEY};
use hyper::StatusCode;

use crate::proxy::http1::websocket_proxy::server_upgrade;
use crate::proxy::proxy_trait::CheckTrait;
use crate::proxy::proxy_trait::CommonCheckRequest;
use http::uri::PathAndQuery;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Body, Request, Response};
use hyper_staticfile::Static;
use hyper_util::rt::TokioIo;
use log::Level;
use prometheus::HistogramTimer;
use rustls_pki_types::CertificateDer;
use serde_json::json;
use std::convert::Infallible;
use std::io::BufReader;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tokio_rustls::TlsAcceptor;
#[derive(Debug)]
pub struct HttpProxy {
    pub port: i32,
    pub channel: mpsc::Receiver<()>,
    pub mapping_key: String,
}

impl HttpProxy {
    pub async fn start_http_server(&mut self) -> Result<(), anyhow::Error> {
        let port_clone = self.port;
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        let client = HttpClients::new();
        let mapping_key_clone1 = self.mapping_key.clone();
        let reveiver = &mut self.channel;

        let listener = TcpListener::bind(addr).await?;
        info!("Listening on http://{}", addr);
        loop {
            tokio::select! {
               Ok((stream,addr))= listener.accept()=>{
                let io = TokioIo::new(stream);
                let client = client.clone();
                let mapping_key2 = mapping_key_clone1.clone();
                tokio::task::spawn(async move {
                    if let Err(err) = http1::Builder::new()
                        .serve_connection(
                            io,
                            service_fn(move |req: Request<Incoming>| {
                                let req = req.map(|item| {
                                    item.map_err(|err| -> Infallible { unreachable!() }).boxed()
                                });
                                proxy_adapter(client.clone(), req, mapping_key2.clone(), addr)
                            }),
                        )
                        .await
                    {
                        println!("Error serving connection: {:?}", err);
                    }
                });
                },
                _ = reveiver.recv() => {
                    break;
                }
            }
        }

        Ok(())
    }
    pub async fn start_https_server(
        &mut self,
        pem_str: String,
        key_str: String,
    ) -> Result<(), anyhow::Error> {
        let port_clone = self.port;
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        let client = HttpClients::new();
        let mapping_key_clone1 = self.mapping_key.clone();

        let mut cer_reader = BufReader::new(pem_str.as_bytes());
        let certs: Vec<CertificateDer<'_>> =
            rustls_pemfile::certs(&mut cer_reader).collect::<Result<Vec<_>, _>>()?;

        let mut key_reader = BufReader::new(key_str.as_bytes());
        let key_der = rustls_pemfile::private_key(&mut key_reader).map(|key| key.unwrap())?;

        let tls_cfg = {
            let cfg = rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, key_der)
                .unwrap();
            Arc::new(cfg)
        };
        let tls_acceptor = TlsAcceptor::from(tls_cfg);
        let reveiver = &mut self.channel;

        let listener = TcpListener::bind(addr).await?;
        info!("Listening on http://{}", addr);
        loop {
            tokio::select! {
                    Ok((tcp_stream,addr))= listener.accept()=>{
                let tls_acceptor = tls_acceptor.clone();

                let client = client.clone();
                let mapping_key2 = mapping_key_clone1.clone();
                tokio::task::spawn(async move {
                    let tls_stream = match tls_acceptor.accept(tcp_stream).await {
                        Ok(tls_stream) => tls_stream,
                        Err(err) => {
                            error!("failed to perform tls handshake: {err:#}");
                            return;
                        }
                    };
                    let io = TokioIo::new(tls_stream);
                    let service = service_fn(move |req: Request<Incoming>| {
                        let req = req
                            .map(|item| item.map_err(|err| -> Infallible { unreachable!() }).boxed());

                        proxy_adapter(client.clone(), req, mapping_key2.clone(), addr)
                    });
                    if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                        error!("Error serving connection: {:?}", err);
                    }
                });
            },
                    _ = reveiver.recv() => {
                        break;
                    }
                }
        }

        Ok(())
    }
}
async fn proxy_adapter(
    client: HttpClients,
    req: Request<BoxBody<Bytes, Infallible>>,
    mapping_key: String,
    remote_addr: SocketAddr,
) -> Result<Response<BoxBody<Bytes, Infallible>>, Infallible> {
    let result = proxy_adapter_with_error(client, req, mapping_key, remote_addr).await;
    match result {
        Ok(res) => Ok(res),
        Err(err) => {
            error!("The error is {}.", err);
            let json_value = json!({
                "error": err.to_string(),
            });
            return Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Full::new(Bytes::copy_from_slice(json_value.to_string().as_bytes())).boxed())
                .unwrap());
        }
    }
}
async fn proxy_adapter_with_error(
    client: HttpClients,
    req: Request<BoxBody<Bytes, Infallible>>,
    mapping_key: String,
    remote_addr: SocketAddr,
) -> Result<Response<BoxBody<Bytes, Infallible>>, anyhow::Error> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri
        .path_and_query()
        .unwrap_or(&PathAndQuery::from_static("/hello?world"))
        .to_string();
    let headers = req.headers().clone();
    let current_time = SystemTime::now();
    let monitor_timer_list = get_timer_list(mapping_key.clone(), path.clone())
        .iter()
        .map(|item| item.start_timer())
        .collect::<Vec<HistogramTimer>>();
    let res = proxy(
        client,
        req,
        mapping_key.clone(),
        remote_addr,
        CommonCheckRequest {},
    )
    .await
    .unwrap_or_else(|err| {
        error!("The error is {}.", err);
        let json_value = json!({
            "response_code": -1,
            "response_object": format!("{}", err)
        });
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Full::new(Bytes::copy_from_slice(json_value.to_string().as_bytes())).boxed())
            .unwrap()
    });
    let mut elapsed_time = 0;
    let elapsed_time_res = current_time.elapsed();
    if let Ok(elapsed_times) = elapsed_time_res {
        elapsed_time = elapsed_times.as_millis();
    }

    let status = res.status().as_u16();
    let json_value: serde_json::Value = format!("{:?}", headers).into();
    monitor_timer_list
        .into_iter()
        .for_each(|item| item.observe_duration());
    inc(mapping_key.clone(), path.clone(), status);

    if log_enabled!(Level::Debug) {
        let (parts, body) = res.into_parts();
        let response_bytes = body
            .collect()
            .await
            .map_err(|_| anyhow!("Can not get bytes from body"))?
            .to_bytes();
        let response_str = String::from_utf8(response_bytes.to_vec())?;
        debug!(target: "app",
           "{}$${}$${}$${}$${}$${}$${}$${:?}",
           remote_addr.to_string(),
           elapsed_time,
           status,
           method.to_string(),
           path,
           json_value.to_string(),
           response_str,
           parts.headers.clone()
        );
        let res = Response::from_parts(parts, Full::new(Bytes::from(response_bytes)).boxed());
        Ok(res)
    } else {
        Ok(res)
    }
}

async fn proxy(
    client: HttpClients,
    mut req: Request<BoxBody<Bytes, Infallible>>,
    mapping_key: String,
    remote_addr: SocketAddr,
    check_trait: impl CheckTrait,
) -> Result<Response<BoxBody<Bytes, Infallible>>, anyhow::Error> {
    debug!("req: {:?}", req);
    let inbound_headers = req.headers().clone();
    let uri = req.uri().clone();
    let check_result = check_trait
        .check_before_request(
            mapping_key.clone(),
            inbound_headers.clone(),
            uri,
            remote_addr,
        )
        .await?;
    if check_result.is_none() {
        return Ok(Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Full::new(Bytes::from(common_constants::DENY_RESPONSE)).boxed())
            .unwrap());
    }
    if inbound_headers.clone().contains_key(CONNECTION)
        && inbound_headers.contains_key(SEC_WEBSOCKET_KEY)
    {
        debug!(
            "The request has been updated to websocket,the req is {:?}!",
            req
        );
        return server_upgrade(req, check_result, client).await;
    }

    if let Some(check_request) = check_result {
        let request_path = check_request.request_path;
        let base_route = check_request.base_route;
        let route = check_request.route;
        if !request_path.clone().contains("http") {
            let mut parts = req.uri().clone().into_parts();
            parts.path_and_query = Some(request_path.try_into().unwrap());
            *req.uri_mut() = Uri::from_parts(parts).unwrap();
            return route_file(base_route, req).await;
            // let res = route_file(base_route, req).await.map(|item| {
            //     item.map(|body| {
            //         let x = body.boxed();
            //     })
            // });
            // return res;
        }
        *req.uri_mut() = request_path
            .parse()
            .map_err(|err: InvalidUri| anyhow!(err.to_string()))?;
        let request_future = if request_path.contains("https") {
            client.request_https(req, DEFAULT_HTTP_TIMEOUT)
        } else {
            client.request_http(req, DEFAULT_HTTP_TIMEOUT)
        };
        let response_result = match request_future.await {
            Ok(response) => response.map_err(|e| anyhow!(e)),
            _ => return Err(anyhow!("Request time out,the uri is {}", request_path)),
        };
        if let (Some(anomaly_detection), Some(liveness_config)) = (
            route.clone().anomaly_detection,
            route.clone().liveness_config,
        ) {
            let is_5xx = match response_result.as_ref() {
                Ok(response) => {
                    let status_code = response.status();
                    status_code.clone().as_u16() >= StatusCode::INTERNAL_SERVER_ERROR.as_u16()
                }
                Err(_) => true,
            };
            let temporary_base_route = base_route.clone();
            let anomaly_detection_status_lock =
                temporary_base_route.anomaly_detection_status.read().await;
            let consecutive_5xx = anomaly_detection_status_lock.consecutive_5xx;
            if is_5xx || consecutive_5xx > 0 {
                if let Err(err) = trigger_anomaly_detection(
                    anomaly_detection,
                    route.liveness_status.clone(),
                    base_route,
                    is_5xx,
                    liveness_config,
                )
                .await
                {
                    error!("{}", err);
                }
            }
        }
        let res = response_result?
            .map(|b| b.boxed())
            .map(|item| item.map_err(|err| -> Infallible { unreachable!() }).boxed());
        return Ok(res);
    }
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Full::new(Bytes::from(common_constants::NOT_FOUND)).boxed())
        .unwrap())
}
async fn trigger_anomaly_detection(
    anomaly_detection: AnomalyDetectionType,
    liveness_status_lock: Arc<RwLock<LivenessStatus>>,
    base_route: BaseRoute,
    is_5xx: bool,
    liveness_config: LivenessConfig,
) -> Result<(), anyhow::Error> {
    let AnomalyDetectionType::Http(http_anomaly_detection_param) = anomaly_detection;
    let res = base_route
        .trigger_http_anomaly_detection(
            http_anomaly_detection_param,
            liveness_status_lock,
            is_5xx,
            liveness_config,
        )
        .await;
    if res.is_err() {
        error!(
            "trigger_http_anomaly_detection error,the error is {}",
            res.unwrap_err()
        );
    }

    Ok(())
}
async fn route_file(
    base_route: BaseRoute,
    req: Request<BoxBody<Bytes, Infallible>>,
) -> Result<Response<BoxBody<Bytes, Infallible>>, anyhow::Error> {
    let static_ = Static::new(Path::new(base_route.endpoint.as_str()));
    let current_res = static_.clone().serve(req).await;
    if current_res.is_ok() {
        let res = current_res.unwrap();
        if res.status() == StatusCode::NOT_FOUND {
            let mut request: Request<()> = Request::default();
            if base_route.try_file.is_none() {
                return Err(anyhow!("Please config the try_file!"));
            }
            *request.uri_mut() = base_route.try_file.unwrap().parse().unwrap();
            return static_
                .clone()
                .serve(request)
                .await
                .map(|item| {
                    item.map(|body| {
                        body.boxed()
                            .map_err(|err| -> Infallible { unreachable!() })
                            .boxed()
                    })
                })
                .map_err(|e| anyhow!(e.to_string()));
        } else {
            return Ok(res.map(|body| {
                body.boxed()
                    .map_err(|err| -> Infallible { unreachable!() })
                    .boxed()
            }));
        }
    }
    let mut request: Request<()> = Request::default();
    if base_route.try_file.is_none() {
        return Err(anyhow!("Please config the try_file!"));
    }
    *request.uri_mut() = base_route.try_file.unwrap().parse().unwrap();
    static_
        .clone()
        .serve(request)
        .await
        .map(|item| {
            item.map(|body| {
                body.boxed()
                    .map_err(|err| -> Infallible { unreachable!() })
                    .boxed()
            })
        })
        .map_err(|e| anyhow!(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration_service::app_config_service::GLOBAL_APP_CONFIG;
    use crate::vojo::allow_deny_ip::AllowDenyObject;
    use crate::vojo::allow_deny_ip::AllowType;

    use crate::configuration_service::app_config_service::GLOBAL_CONFIG_MAPPING;
    use crate::utils::uuid::get_uuid;
    use crate::vojo::anomaly_detection::BaseAnomalyDetectionParam;
    use crate::vojo::anomaly_detection::HttpAnomalyDetectionParam;
    use crate::vojo::api_service_manager::ApiServiceManager;
    use crate::vojo::app_config::ApiService;
    use crate::vojo::app_config::LivenessStatus;
    use crate::vojo::app_config::Matcher;
    use crate::vojo::app_config::Route;
    use crate::vojo::app_config::ServiceConfig;
    use crate::vojo::base_response::BaseResponse;
    use crate::vojo::route::AnomalyDetectionStatus;
    use crate::vojo::route::{BaseRoute, LoadbalancerStrategy, RandomBaseRoute, RandomRoute};
    use lazy_static::lazy_static;
    use regex::Regex;
    use std::env;
    use std::fs::File;
    use std::io::BufReader;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::Arc;
    use std::{thread, time};
    use tokio::runtime::{Builder, Runtime};
    use tokio::sync::RwLock;

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
    fn test_output_serde() {
        let re = Regex::new("/v1/proxy").unwrap();
        let caps1 = re.captures("/v1/proxy");
        let caps2 = re.captures("/v1/proxy/api");
        let caps3 = re.captures("/v1/proxy/api?test=1");
        let caps4 = re.captures("/v1/prox");
        assert!(caps1.is_some(),);
        assert!(caps2.is_some(),);
        assert!(caps3.is_some(),);
        assert!(caps4.is_none());
    }
    #[test]
    fn test_certificate() {
        let current_dir = env::current_dir()
            .unwrap()
            .join("config")
            .join("test_cert.pem");
        let file = File::open(current_dir).unwrap();
        let mut reader = BufReader::new(file);
        let mut certs_result = rustls_pemfile::certs(&mut reader);
        let first = certs_result.next().unwrap();
        assert!(first.is_ok());
    }
    #[test]
    fn test_private_key() {
        let current_dir = env::current_dir()
            .unwrap()
            .join("config")
            .join("test_key.pem");
        let data = std::fs::read_to_string(current_dir).unwrap();

        println!("input: {:?}", data);
        let result_doc = pkcs8::PrivateKeyDocument::from_pem(&data);
        assert!(result_doc.is_ok());
    }
    #[test]
    fn test_http_client_ok() {
        TOKIO_RUNTIME.spawn(async {
            let (_, receiver) = tokio::sync::mpsc::channel(10);

            let mut http_proxy = HttpProxy {
                port: 9987,
                channel: receiver,
                mapping_key: String::from("random key"),
            };
            let _result = http_proxy.start_http_server().await;
        });
        let sleep_time = time::Duration::from_millis(100);
        thread::sleep(sleep_time);
        TOKIO_RUNTIME.spawn(async {
            let client = HttpClients::new();
            let request = Request::builder()
                .uri("http://127.0.0.1:9987/get")
                .body(Full::new(Bytes::from("value")).boxed())
                .unwrap();
            let response_result = client.request_http(request, 5).await;
            assert!(response_result.is_ok());
            let response = response_result.unwrap().unwrap();
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let body_bytes = response.collect().await.unwrap().to_bytes();
            // let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
            let base_response: BaseResponse<String> = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(base_response.response_code, -1);
        });
        let sleep_time2 = time::Duration::from_millis(100);
        thread::sleep(sleep_time2);
    }
    #[test]
    fn test_https_client_ok() {
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

        TOKIO_RUNTIME.spawn(async {
            let (_, receiver) = tokio::sync::mpsc::channel(10);

            let mut http_proxy = HttpProxy {
                port: 4450,
                channel: receiver,
                mapping_key: String::from("random key"),
            };
            let _result = http_proxy
                .start_https_server(ca_certificate, private_key)
                .await;
        });
        let sleep_time = time::Duration::from_millis(100);
        thread::sleep(sleep_time);
        TOKIO_RUNTIME.spawn(async {
            let client = HttpClients::new();
            let request = Request::builder()
                .uri("https://localhost:4450/get")
                .body(Full::new(Bytes::new()).boxed())
                .unwrap();
            let response_result = client.request_https(request, 5).await;
            assert!(response_result.is_ok());
            let response = response_result.unwrap().unwrap();
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let body_bytes = response.collect().await.unwrap().to_bytes();
            println!("{:?}", body_bytes);
            let base_response: BaseResponse<String> = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(base_response.response_code, -1);
        });
        let sleep_time2 = time::Duration::from_millis(100);
        thread::sleep(sleep_time2);
    }
    #[test]
    fn test_proxy_adapter_error() {
        TOKIO_RUNTIME.spawn(async {
            let client = HttpClients::new();
            let request = Request::builder()
                .uri("https://localhost:4450/get")
                .body(Full::new(Bytes::new()).boxed())
                .unwrap();
            let mapping_key = String::from("test");
            let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
            let res = proxy_adapter(client, request, mapping_key, socket).await;
            assert!(res.is_ok());
        });
    }
    #[test]
    fn test_proxy_error() {
        TOKIO_RUNTIME.spawn(async {
            let client = HttpClients::new();
            let request = Request::builder()
                .uri("http://localhost:4450/get")
                .body(Full::new(Bytes::new()).boxed())
                .unwrap();
            let mapping_key = String::from("test");
            let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
            let res = proxy(client, request, mapping_key, socket, CommonCheckRequest {}).await;
            assert!(res.is_err());
        });
    }
    #[test]
    fn test_route_file_error() {
        TOKIO_RUNTIME.spawn(async {
            let request = Request::builder()
                .uri("http://localhost:4450/get")
                .body(Full::new(Bytes::new()).boxed())
                .unwrap();
            let base_route = BaseRoute {
                endpoint: String::from("not_found"),
                try_file: None,
                is_alive: Arc::new(RwLock::new(None)),
                anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                    consecutive_5xx: 100,
                })),
            };
            let res = route_file(base_route, request).await;
            assert!(res.is_err());
        });

        let sleep_time = time::Duration::from_millis(100);
        thread::sleep(sleep_time);
    }
    #[test]
    fn test_route_file_ok() {
        TOKIO_RUNTIME.spawn(async {
            let request = Request::builder()
                .uri("http://localhost:4450/app_config.yaml")
                .body(Full::new(Bytes::new()).boxed())
                .unwrap();
            let base_route = BaseRoute {
                endpoint: String::from("config"),
                try_file: None,
                is_alive: Arc::new(RwLock::new(None)),
                anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                    consecutive_5xx: 100,
                })),
            };
            let res = route_file(base_route, request).await;
            assert!(res.is_ok());
        });
    }
    #[test]
    fn test_route_file_with_try_file_ok() {
        TOKIO_RUNTIME.spawn(async {
            let request = Request::builder()
                .uri("http://localhost:4450/xxxxxx")
                .body(Full::new(Bytes::new()).boxed())
                .unwrap();
            let base_route = BaseRoute {
                endpoint: String::from("config"),
                try_file: Some(String::from("app_config.yaml")),
                is_alive: Arc::new(RwLock::new(None)),
                anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                    consecutive_5xx: 100,
                })),
            };
            let res = route_file(base_route, request).await;
            assert!(res.is_ok());
        });
    }

    #[test]
    fn test_proxy_allow_all() {
        TOKIO_RUNTIME.block_on(async {
            let route = LoadbalancerStrategy::Random(RandomRoute {
                routes: vec![RandomBaseRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("http://httpbin.org:80"),
                        try_file: None,
                        is_alive: Arc::new(RwLock::new(None)),
                        anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        })),
                    },
                }],
            });
            let (sender, _) = tokio::sync::mpsc::channel(10);

            let api_service_manager = ApiServiceManager {
                sender,
                service_config: ServiceConfig {
                    key_str: None,
                    server_type: crate::vojo::app_config::ServiceType::Http,
                    cert_str: None,
                    routes: vec![Route {
                        rewrite_headers: None,
                        host_name: None,
                        route_id: get_uuid(),
                        matcher: Some(Matcher {
                            prefix: String::from("/"),
                            prefix_rewrite: String::from("test"),
                        }),
                        route_cluster: route,
                        allow_deny_list: Some(vec![AllowDenyObject {
                            limit_type: AllowType::AllowAll,
                            value: None,
                        }]),
                        authentication: None,
                        anomaly_detection: None,
                        liveness_config: None,
                        liveness_status: Arc::new(RwLock::new(LivenessStatus {
                            current_liveness_count: 0,
                        })),
                        ratelimit: None,
                        health_check: None,
                    }],
                },
            };
            let mut write = GLOBAL_APP_CONFIG.write().await;
            write.api_service_config.push(ApiService {
                api_service_id: get_uuid(),
                listen_port: 9998,
                service_config: api_service_manager.service_config.clone(),
            });
            GLOBAL_CONFIG_MAPPING.insert(String::from("9998-HTTP"), api_service_manager);
            let client = HttpClients::new();
            let request = Request::builder()
                .uri("http://localhost:4450/get")
                .body(Full::new(Bytes::new()).boxed())
                .unwrap();
            let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
            let res = proxy(
                client,
                request,
                String::from("9998-HTTP"),
                socket,
                CommonCheckRequest {},
            )
            .await;
            assert!(res.is_ok());
        });
    }
    #[test]
    fn test_proxy_deny_ip() {
        TOKIO_RUNTIME.block_on(async {
            let route = LoadbalancerStrategy::Random(RandomRoute {
                routes: vec![RandomBaseRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("httpbin.org:80"),
                        try_file: None,
                        is_alive: Arc::new(RwLock::new(None)),
                        anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        })),
                    },
                }],
            });
            let (sender, _) = tokio::sync::mpsc::channel(10);

            let api_service_manager = ApiServiceManager {
                sender,
                service_config: ServiceConfig {
                    key_str: None,
                    server_type: crate::vojo::app_config::ServiceType::Tcp,
                    cert_str: None,
                    routes: vec![Route {
                        rewrite_headers: None,
                        route_id: get_uuid(),
                        host_name: None,
                        matcher: Some(Matcher {
                            prefix: String::from("/"),
                            prefix_rewrite: String::from("test"),
                        }),
                        route_cluster: route,
                        allow_deny_list: Some(vec![AllowDenyObject {
                            limit_type: AllowType::Deny,
                            value: Some(String::from("127.0.0.1")),
                        }]),
                        authentication: None,
                        ratelimit: None,
                        liveness_status: Arc::new(RwLock::new(LivenessStatus {
                            current_liveness_count: 0,
                        })),
                        health_check: None,
                        anomaly_detection: None,
                        liveness_config: None,
                    }],
                },
            };
            let mut write = GLOBAL_APP_CONFIG.write().await;
            write.api_service_config.push(ApiService {
                api_service_id: get_uuid(),
                listen_port: 9999,
                service_config: api_service_manager.service_config.clone(),
            });
            GLOBAL_CONFIG_MAPPING.insert(String::from("9999-HTTP"), api_service_manager);
            let client = HttpClients::new();
            let request = Request::builder()
                .uri("http://localhost:4450/get")
                .body(Full::new(Bytes::new()).boxed())
                .unwrap();
            let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
            let res = proxy(
                client,
                request,
                String::from("9999-HTTP"),
                socket,
                CommonCheckRequest {},
            )
            .await;
            assert!(res.is_ok());
            let response = res.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        });
    }
    #[test]
    fn test_proxy_turn_5xx() {
        TOKIO_RUNTIME.block_on(async {
            let route = LoadbalancerStrategy::Random(RandomRoute {
                routes: vec![RandomBaseRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("http://127.0.0.1:9851"),
                        try_file: None,
                        is_alive: Arc::new(RwLock::new(None)),
                        anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        })),
                    },
                }],
            });
            let (sender, _) = tokio::sync::mpsc::channel(10);

            let api_service_manager = ApiServiceManager {
                sender,
                service_config: ServiceConfig {
                    key_str: None,
                    server_type: crate::vojo::app_config::ServiceType::Http,
                    cert_str: None,
                    routes: vec![Route {
                        rewrite_headers: None,
                        host_name: None,
                        route_id: get_uuid(),
                        matcher: Some(Matcher {
                            prefix: String::from("/"),
                            prefix_rewrite: String::from("test"),
                        }),
                        route_cluster: route,
                        allow_deny_list: None,
                        authentication: None,
                        anomaly_detection: Some(AnomalyDetectionType::Http(
                            HttpAnomalyDetectionParam {
                                consecutive_5xx: 3,
                                base_anomaly_detection_param: BaseAnomalyDetectionParam {
                                    ejection_second: 10,
                                },
                            },
                        )),
                        liveness_config: Some(LivenessConfig {
                            min_liveness_count: 1,
                        }),
                        liveness_status: Arc::new(RwLock::new(LivenessStatus {
                            current_liveness_count: 0,
                        })),
                        ratelimit: None,
                        health_check: None,
                    }],
                },
            };
            let mut write = GLOBAL_APP_CONFIG.write().await;
            write.api_service_config.push(ApiService {
                api_service_id: get_uuid(),
                listen_port: 10024,
                service_config: api_service_manager.service_config.clone(),
            });
            GLOBAL_CONFIG_MAPPING.insert(String::from("10024-HTTP"), api_service_manager);
            let client = HttpClients::new();
            let request = Request::builder()
                .uri("http://localhost:10024/get")
                .body(Full::new(Bytes::new()).boxed())
                .unwrap();
            let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
            let res = proxy(
                client,
                request,
                String::from("10024-HTTP"),
                socket,
                CommonCheckRequest {},
            )
            .await;
            assert!(res.is_err());
        });
    }
}
