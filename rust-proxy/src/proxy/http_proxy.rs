use crate::configuration_service::app_config_servive::GLOBAL_CONFIG_MAPPING;
use crate::proxy::tls_acceptor::TlsAcceptor;
use http::StatusCode;
use hyper::client::HttpConnector;
use hyper::server::conn::AddrIncoming;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Request, Response, Server};
use hyper_rustls::ConfigBuilderExt;
use regex::Regex;
use std::convert::Infallible;
use std::env;
use std::io::BufReader;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::File;
use tokio::sync::mpsc;
use tokio_util::codec::{BytesCodec, FramedRead};

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
            .with_native_roots()
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
                    proxy(client.clone(), req, mapping_key2.clone())
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
    pub async fn start_https_server(&mut self) {
        let port_clone = self.port.clone();
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        let client = Clients::new();
        let mapping_key_clone1 = self.mapping_key.clone();

        let make_service = make_service_fn(move |_| {
            let client = client.clone();
            let mapping_key2 = mapping_key_clone1.clone();

            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    proxy(client.clone(), req, mapping_key2.clone())
                }))
            }
        });
        let mapping_key3 = self.mapping_key.clone();
        let service_config = GLOBAL_CONFIG_MAPPING
            .get(&mapping_key3)
            .unwrap()
            .service_config
            .clone();
        let pem_str = service_config.cert_str.unwrap();
        let key_str = service_config.key_str.unwrap();
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
async fn proxy(
    client: Clients,
    mut req: Request<Body>,
    mapping_key: String,
) -> Result<Response<Body>, hyper::Error> {
    debug!("req: {:?}", req);

    let backend_path = req.uri().path();
    let api_service_manager = GLOBAL_CONFIG_MAPPING.get(&mapping_key).unwrap().clone();

    for item in api_service_manager.service_config.routes {
        let match_prefix = item.matcher.prefix;
        let re = Regex::new(match_prefix.as_str()).unwrap();
        let match_res = re.captures(backend_path);
        if match_res.is_some() {
            let route_cluster = item.route_cluster.clone();
            if !route_cluster.clone().contains("http") {
                return route_file(route_cluster, String::from(backend_path)).await;
            }
            let request_path = format!("{}{}", route_cluster, match_prefix.clone());
            *req.uri_mut() = request_path.parse().unwrap();
            if request_path.contains("https") {
                return client.request_https(req).await;
            } else {
                return client.request_http(req).await;
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
    resource_dir: String,
    request_path: String,
) -> Result<Response<Body>, hyper::Error> {
    let app_dir = env::current_dir().unwrap();
    let resource = get_file_path(resource_dir);
    let request_file_path = get_file_path(request_path);
    let file_name = app_dir.join(resource).join(request_file_path);
    if let Ok(file) = File::open(file_name).await {
        let stream = FramedRead::new(file, BytesCodec::new());
        let body = Body::wrap_stream(stream);
        return Ok(Response::new(body));
    }

    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Body::from(
            r#"{
            "response_code": -1,
            "response_object": "The file can not be found!"
        }"#,
        ))
        .unwrap())
}
fn get_file_path(url: String) -> PathBuf {
    let mut res = PathBuf::new();
    url.split("/").into_iter().for_each(|s| {
        let current_dir = res.clone();
        res = current_dir.join(s);
    });
    return res;
}
#[cfg(test)]
mod tests {
    use regex::Regex;
    use std::env;
    use std::fs::File;
    use std::io::BufReader;
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
}
