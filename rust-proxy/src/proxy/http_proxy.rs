use crate::configuration_service::app_config_service::GLOBAL_CONFIG_MAPPING;
use crate::proxy::tls_acceptor::TlsAcceptor;
use crate::vojo::route::BaseRoute;
use http::StatusCode;
use hyper::client::HttpConnector;
use hyper::server::conn::AddrIncoming;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Request, Response, Server};
use hyper_rustls::ConfigBuilderExt;
use hyper_staticfile::Static;
use regex::Regex;
use serde_json::json;
use std::convert::Infallible;
use std::io::BufReader;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug)]
pub struct GeneralError(pub anyhow::Error);
impl std::error::Error for GeneralError {}
impl std::fmt::Display for GeneralError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_string())
    }
}
impl GeneralError {
    pub fn from(err: hyper::Error) -> Self {
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
    fn new() -> Clients {
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
        return Clients {
            http_client: http_client,
            https_client: https_client,
        };
    }
    async fn request_http(&self, req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        return self.http_client.request(req).await;
    }
    async fn request_https(&self, req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        return self.https_client.request(req).await;
    }
}

impl HttpProxy {
    pub async fn start_http_server(&mut self) {
        let port_clone = self.port.clone();
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        let client = Clients::new();
        let mapping_key_clone1 = self.mapping_key.clone();
        let make_service = make_service_fn(move |_| {
            let client = client.clone();
            let mapping_key2 = mapping_key_clone1.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    proxy_adapter(client.clone(), req, mapping_key2.clone())
                }))
            }
        });
        let server = Server::bind(&addr)
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
    }
    pub async fn start_https_server(&mut self, pem_str: String, key_str: String) {
        let port_clone = self.port.clone();
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        let client = Clients::new();
        let mapping_key_clone1 = self.mapping_key.clone();

        let make_service = make_service_fn(move |_| {
            let client = client.clone();
            let mapping_key2 = mapping_key_clone1.clone();

            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    proxy_adapter(client.clone(), req, mapping_key2.clone())
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
        let incoming = AddrIncoming::bind(&addr).unwrap();
        let server = Server::builder(TlsAcceptor::new(tls_cfg, incoming)).serve(make_service);
        info!("Listening on https://{}", addr);

        let reveiver = &mut self.channel;

        let graceful = server.with_graceful_shutdown(async move {
            reveiver.recv().await;
        });

        if let Err(e) = graceful.await {
            info!("server has receive error: {}", e);
        }
    }
}
async fn proxy_adapter(
    client: Clients,
    req: Request<Body>,
    mapping_key: String,
) -> Result<Response<Body>, Infallible> {
    match proxy(client, req, mapping_key).await {
        Ok(r) => Ok(r),
        Err(err) => {
            let json_value = json!({
                "response_code": -1,
                "response_object": format!("{}", err.to_string())
            });
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(json_value.to_string()))
                .unwrap())
        }
    }
}
async fn proxy(
    client: Clients,
    mut req: Request<Body>,
    mapping_key: String,
) -> Result<Response<Body>, GeneralError> {
    debug!("req: {:?}", req);

    let backend_path = req.uri().path();
    let api_service_manager = match GLOBAL_CONFIG_MAPPING.get(&mapping_key) {
        Some(r) => r.clone(),
        None => {
            return Err(GeneralError(anyhow!(format!(
                "Can not find the config mapping on the key {}!",
                mapping_key.clone()
            ))))
        }
    };

    for item in api_service_manager.service_config.routes {
        let match_prefix = item.matcher.prefix;
        let re = Regex::new(match_prefix.as_str()).unwrap();
        let match_res = re.captures(backend_path);
        if match_res.is_some() {
            let route_cluster = match item.route_cluster.clone().get_route() {
                Ok(r) => r,
                Err(err) => return Err(GeneralError(anyhow!(err.to_string()))),
            };
            let endpoint = route_cluster.clone().endpoint;
            if !endpoint.clone().contains("http") {
                return route_file(route_cluster, req).await;
            }
            let request_path = format!("{}{}", endpoint, match_prefix.clone());
            *req.uri_mut() = request_path.parse().unwrap();
            if request_path.contains("https") {
                return client.request_https(req).await.map_err(GeneralError::from);
            } else {
                return client.request_http(req).await.map_err(GeneralError::from);
            }
        }
    }
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from(
            r#"{
            "response_code": -1,
            "response_object": "The route could not be found in the Proxy!"
        }"#,
        ))
        .unwrap())
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
    return static_
        .clone()
        .serve(request)
        .await
        .map_err(|e| GeneralError(anyhow!(e.to_string())));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vojo::vojo::BaseResponse;
    use lazy_static::lazy_static;
    use regex::Regex;
    use std::env;
    use std::fs::File;
    use std::io::BufReader;
    use std::{thread, time};
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
    fn test_output_serde() {
        let re = Regex::new("/v1/proxy").unwrap();
        let caps1 = re.captures("/v1/proxy");
        let caps2 = re.captures("/v1/proxy/api");
        let caps3 = re.captures("/v1/proxy/api?test=1");
        let caps4 = re.captures("/v1/prox");
        assert_eq!(caps1.is_some(), true);
        assert_eq!(caps2.is_some(), true);
        assert_eq!(caps3.is_some(), true);
        assert_eq!(caps4.is_some(), false);
    }
    #[test]
    fn test_certificate() {
        let current_dir = env::current_dir()
            .unwrap()
            .join("config")
            .join("cacert.pem");
        let file = File::open(current_dir).unwrap();
        let mut reader = BufReader::new(file);
        let certs_result = rustls_pemfile::certs(&mut reader);
        assert_eq!(certs_result.is_err(), false);

        let cert = certs_result.unwrap();
        assert_eq!(cert.len(), 1);
    }
    #[test]
    fn test_private_key() {
        let current_dir = env::current_dir()
            .unwrap()
            .join("config")
            .join("privkey.pem");
        let data = std::fs::read_to_string(current_dir).unwrap();

        println!("input: {:?}", data);
        let result_doc = pkcs8::PrivateKeyDocument::from_pem(&data);
        assert_eq!(result_doc.is_ok(), true);
        rustls::PrivateKey(result_doc.unwrap().as_ref().to_owned());
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
            http_proxy.start_http_server().await;
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
            assert_eq!(response_result.is_ok(), true);
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
            .join("privkey.pem");
        let private_key = std::fs::read_to_string(private_key_path).unwrap();

        let ca_certificate_path = env::current_dir()
            .unwrap()
            .join("config")
            .join("privkey.pem");
        let ca_certificate = std::fs::read_to_string(ca_certificate_path).unwrap();

        TOKIO_RUNTIME.spawn(async {
            let (_, receiver) = tokio::sync::mpsc::channel(10);

            let mut http_proxy = HttpProxy {
                port: 4450,
                channel: receiver,
                mapping_key: String::from("random key"),
            };
            http_proxy
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
            assert_eq!(response_result.is_ok(), true);
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
            let res = proxy_adapter(client, request, mapping_key).await;
            assert_eq!(res.is_ok(), true);
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
            let res = proxy(client, request, mapping_key).await;
            assert_eq!(res.is_err(), true);
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
                weight: 10,
                try_file: None,
            };
            let res = route_file(base_route, request).await;
            assert_eq!(res.is_err(), true);
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
                weight: 10,
                try_file: None,
            };
            let res = route_file(base_route, request).await;
            assert_eq!(res.is_ok(), true);
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
                weight: 10,
                try_file: Some(String::from("app_config.yaml")),
            };
            let res = route_file(base_route, request).await;
            assert_eq!(res.is_ok(), true);
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
            let error_message = err.to_string().clone();
            let general_error = GeneralError::from(err);
            assert_eq!(error_message, general_error.to_string());
        });
        let sleep_time = time::Duration::from_millis(1000);
        thread::sleep(sleep_time);
    }
}
