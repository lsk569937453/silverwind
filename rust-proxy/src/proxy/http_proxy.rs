use crate::configuration_service::app_config_service::GLOBAL_CONFIG_MAPPING;

use crate::constants::common_constants;
use crate::constants::common_constants::DEFAULT_HTTP_TIMEOUT;
use crate::monitor::prometheus_exporter::{get_timer_list, inc};
use crate::proxy::tls_acceptor::TlsAcceptor;
use crate::proxy::tls_stream::TlsStream;
use crate::vojo::anomaly_detection::AnomalyDetectionType;
use crate::vojo::app_config::{LivenessConfig, LivenessStatus};
use crate::vojo::route::BaseRoute;
use http::uri::InvalidUri;
use http::{StatusCode, Uri};
use hyper::client::HttpConnector;
use hyper::client::ResponseFuture;
use hyper::server::conn::AddrIncoming;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Request, Response, Server};
use hyper_rustls::ConfigBuilderExt;
use hyper_staticfile::Static;
use log::Level;
use prometheus::HistogramTimer;
use serde_json::json;
use std::convert::Infallible;
use std::io::BufReader;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tokio::time::timeout;
use url::Url;
#[derive(Debug)]
pub struct GeneralError(pub anyhow::Error);
impl std::error::Error for GeneralError {}
impl std::fmt::Display for GeneralError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl GeneralError {
    pub fn _from(err: hyper::Error) -> Self {
        GeneralError(anyhow!(err.to_string()))
    }
}
#[derive(Debug)]
pub struct HttpProxy {
    pub port: i32,
    pub channel: mpsc::Receiver<()>,
    pub mapping_key: String,
}
#[derive(Clone)]
pub struct Clients {
    pub http_client: Client<HttpConnector>,
    pub https_client: Client<hyper_rustls::HttpsConnector<HttpConnector>>,
}
impl Clients {
    pub fn new() -> Clients {
        let http_client = Client::builder()
            .http1_title_case_headers(true)
            .http1_preserve_header_case(true)
            .build_http();

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
        Clients {
            http_client,
            https_client,
        }
    }
    pub fn request_http(&self, req: Request<Body>) -> ResponseFuture {
        self.http_client.request(req)
    }
    pub fn request_https(&self, req: Request<Body>) -> ResponseFuture {
        self.https_client.request(req)
    }
}

impl HttpProxy {
    pub async fn start_http_server(&mut self) -> Result<(), anyhow::Error> {
        let port_clone = self.port;
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        let client = Clients::new();
        let mapping_key_clone1 = self.mapping_key.clone();
        let make_service = make_service_fn(move |socket: &AddrStream| {
            let client = client.clone();
            let mapping_key2 = mapping_key_clone1.clone();
            let remote_addr = socket.remote_addr();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    proxy_adapter(client.clone(), req, mapping_key2.clone(), remote_addr)
                }))
            }
        });
        let server = Server::try_bind(&addr)
            .map_err(|e| {
                anyhow!(
                    "Cause error when binding the socket,the addr is {},the error is {}.",
                    addr.clone(),
                    e.to_string()
                )
            })?
            .http1_preserve_header_case(true)
            .http1_title_case_headers(true)
            .serve(make_service);
        info!("Listening on http://{}", addr);

        let reveiver = &mut self.channel;

        let graceful = server.with_graceful_shutdown(async move {
            reveiver.recv().await;
        });
        if let Err(e) = graceful.await {
            info!("server has receive error: {}", e);
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
        let client = Clients::new();
        let mapping_key_clone1 = self.mapping_key.clone();

        let make_service = make_service_fn(move |socket: &TlsStream| {
            let client = client.clone();
            let mapping_key2 = mapping_key_clone1.clone();
            let remote_addr = socket.remote_addr();

            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    proxy_adapter(client.clone(), req, mapping_key2.clone(), remote_addr)
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
        let server = Server::builder(TlsAcceptor::new(tls_cfg, incoming)).serve(make_service);
        info!("Listening on https://{}", addr);

        let reveiver = &mut self.channel;

        let graceful = server.with_graceful_shutdown(async move {
            reveiver.recv().await;
        });

        if let Err(e) = graceful.await {
            info!("server has receive error: {}", e);
        }
        Ok(())
    }
}

async fn proxy_adapter(
    client: Clients,
    req: Request<Body>,
    mapping_key: String,
    remote_addr: SocketAddr,
) -> Result<Response<Body>, Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path();
    let headers = req.headers().clone();
    let current_time = SystemTime::now();
    let monitor_timer_list = get_timer_list(mapping_key.clone(), String::from(path))
        .iter()
        .map(|item| item.start_timer())
        .collect::<Vec<HistogramTimer>>();
    let res = proxy(client, req, mapping_key.clone(), remote_addr)
        .await
        .unwrap_or_else(|err| {
            let json_value = json!({
                "response_code": -1,
                "response_object": format!("{}", err)
            });
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(json_value.to_string()))
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
    inc(mapping_key.clone(), String::from(path), status);
    info!(target: "app",
        "{}$${}$${}$${}$${}$${}",
        remote_addr.to_string(),
        elapsed_time,
        status,
        method.to_string(),
        path,
        json_value.to_string()
    );
    Ok(res)
}

async fn proxy(
    client: Clients,
    mut req: Request<Body>,
    mapping_key: String,
    remote_addr: SocketAddr,
) -> Result<Response<Body>, GeneralError> {
    if log_enabled!(Level::Debug) {
        debug!("req: {:?}", req);
    }

    let backend_path = req.uri().path();
    let api_service_manager = GLOBAL_CONFIG_MAPPING
        .get(&mapping_key)
        .ok_or(GeneralError(anyhow!(format!(
            "Can not find the config mapping on the key {}!",
            mapping_key.clone()
        ))))?
        .clone();
    let addr_string = remote_addr.ip().to_string();
    for item in api_service_manager.service_config.routes {
        let match_result = item
            .is_matched(backend_path, Some(req.headers().clone()))
            .map_err(GeneralError)?;
        if match_result.clone().is_none() {
            continue;
        }
        let rest_path = match_result.unwrap();

        let is_allowed = item
            .is_allowed(addr_string.clone(), Some(req.headers().clone()))
            .await
            .map_err(|err| GeneralError(anyhow!(err.to_string())))?;
        if !is_allowed {
            return Ok(Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Body::from(common_constants::DENY_RESPONSE))
                .unwrap());
        }

        let base_route = item
            .route_cluster
            .clone()
            .get_route(req.headers().clone())
            .await
            .map_err(|err| GeneralError(anyhow!(err.to_string())))?;
        let endpoint = base_route.clone().endpoint;
        if !endpoint.clone().contains("http") {
            let mut parts = req.uri().clone().into_parts();
            parts.path_and_query = Some(rest_path.try_into().unwrap());
            *req.uri_mut() = Uri::from_parts(parts).unwrap();
            return route_file(base_route, req).await;
        }
        let host =
            Url::parse(endpoint.as_str()).map_err(|err| GeneralError(anyhow!(err.to_string())))?;

        let request_path = host
            .join(rest_path.clone().as_str())
            .map_err(|err| GeneralError(anyhow!(err.to_string())))?
            .to_string();
        *req.uri_mut() = request_path
            .parse()
            .map_err(|err: InvalidUri| GeneralError(anyhow!(err.to_string())))?;
        let request_future = if request_path.contains("https") {
            client.request_https(req)
        } else {
            client.request_http(req)
        };
        let response_result =
            match timeout(Duration::from_secs(DEFAULT_HTTP_TIMEOUT), request_future).await {
                Ok(response) => response.map_err(|e| GeneralError(anyhow!(e.to_string()))),
                Err(_) => Err(GeneralError(anyhow!(
                    "Request time out,the uri is {}",
                    request_path
                ))),
            };
        if let (Some(anomaly_detection), Some(liveness_config)) =
            (item.clone().anomaly_detection, item.clone().liveness_config)
        {
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
                    item.liveness_status.clone(),
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
        return response_result;
    }
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from(common_constants::NOT_FOUND))
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
    req: Request<Body>,
) -> Result<Response<Body>, GeneralError> {
    let static_ = Static::new(Path::new(base_route.endpoint.as_str()));
    let current_res = static_.clone().serve(req).await;
    if current_res.is_ok() {
        let res = current_res.unwrap();
        if res.status() == StatusCode::NOT_FOUND {
            let mut request: Request<()> = Request::default();
            if base_route.try_file.is_none() {
                return Err(GeneralError(anyhow!("Please config the try_file!")));
            }
            *request.uri_mut() = base_route.try_file.unwrap().parse().unwrap();
            return static_
                .clone()
                .serve(request)
                .await
                .map_err(|e| GeneralError(anyhow!(e.to_string())));
        } else {
            return Ok(res);
        }
    }
    let mut request: Request<()> = Request::default();
    if base_route.try_file.is_none() {
        return Err(GeneralError(anyhow!("Please config the try_file!")));
    }
    *request.uri_mut() = base_route.try_file.unwrap().parse().unwrap();
    static_
        .clone()
        .serve(request)
        .await
        .map_err(|e| GeneralError(anyhow!(e.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration_service::app_config_service::GLOBAL_APP_CONFIG;
    use crate::vojo::allow_deny_ip::AllowDenyObject;
    use crate::vojo::allow_deny_ip::AllowType;

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
        let certs_result = rustls_pemfile::certs(&mut reader);
        assert!(certs_result.is_ok());

        let cert = certs_result.unwrap();
        assert_eq!(cert.len(), 1);
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
            let client = Clients::new();
            let request = Request::builder()
                .uri("http://127.0.0.1:9987/get")
                .body(Body::empty())
                .unwrap();
            let response_result = client.request_http(request).await;
            assert!(response_result.is_ok());
            let response = response_result.unwrap();
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
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
            let client = Clients::new();
            let request = Request::builder()
                .uri("https://localhost:4450/get")
                .body(Body::empty())
                .unwrap();
            let response_result = client.request_https(request).await;
            assert!(response_result.is_ok());
            let response = response_result.unwrap();
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
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
            let client = Clients::new();
            let request = Request::builder()
                .uri("https://localhost:4450/get")
                .body(Body::empty())
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
            let client = Clients::new();
            let request = Request::builder()
                .uri("http://localhost:4450/get")
                .body(Body::empty())
                .unwrap();
            let mapping_key = String::from("test");
            let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
            let res = proxy(client, request, mapping_key, socket).await;
            assert!(res.is_err());
        });
    }
    #[test]
    fn test_route_file_error() {
        TOKIO_RUNTIME.spawn(async {
            let request = Request::builder()
                .uri("http://localhost:4450/get")
                .body(Body::empty())
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
                .body(Body::empty())
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
                .body(Body::empty())
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
    fn test_generate_error_ok() {
        TOKIO_RUNTIME.spawn(async {
            let request = hyper::Request::builder()
                .method(hyper::Method::POST)
                .uri("http://xxtpbin.org/xxx")
                .header("content-type", "application/json")
                .body(hyper::Body::from(r#"{"library":"hyper"}"#))
                .unwrap();
            let client = hyper::Client::new();
            let response = client.request(request).await;
            let err = response.unwrap_err();
            let error_message = err.to_string();
            let general_error = GeneralError::_from(err);
            assert_eq!(error_message, general_error.to_string());
        });
        let sleep_time = time::Duration::from_millis(1000);
        thread::sleep(sleep_time);
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
            let client = Clients::new();
            let request = Request::builder()
                .uri("http://localhost:4450/get")
                .body(Body::empty())
                .unwrap();
            let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
            let res = proxy(client, request, String::from("9998-HTTP"), socket).await;
            assert!(res.is_ok());
            // assert_eq!(res.unwrap_err().to_string(), String::from("invalid format"));
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
            let client = Clients::new();
            let request = Request::builder()
                .uri("http://localhost:4450/get")
                .body(Body::empty())
                .unwrap();
            let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
            let res = proxy(client, request, String::from("9999-HTTP"), socket).await;
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
            let client = Clients::new();
            let request = Request::builder()
                .uri("http://localhost:10024/get")
                .body(Body::empty())
                .unwrap();
            let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
            let res = proxy(client, request, String::from("10024-HTTP"), socket).await;
            assert!(res.is_err());
        });
    }
}
