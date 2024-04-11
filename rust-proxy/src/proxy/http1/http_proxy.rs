use crate::constants::common_constants;
use crate::constants::common_constants::DEFAULT_HTTP_TIMEOUT;
use crate::monitor::prometheus_exporter::{get_timer_list, inc};
use crate::proxy::http1::http_client::HttpClients;

use crate::vojo::anomaly_detection::AnomalyDetectionType;
use crate::vojo::app_config::{LivenessConfig, LivenessStatus};
use crate::vojo::app_error::AppError;
use crate::vojo::route::BaseRoute;
use bytes::Bytes;
use http::uri::InvalidUri;
use http::Uri;
use hyper::body::Incoming;
use hyper::header::{CONNECTION, SEC_WEBSOCKET_KEY};
use hyper::StatusCode;

use crate::proxy::http1::websocket_proxy::server_upgrade;
use crate::proxy::proxy_trait::CheckTrait;
use crate::proxy::proxy_trait::CommonCheckRequest;
use crate::vojo::app_config::AppConfig;
use http::uri::PathAndQuery;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
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
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio_rustls::TlsAcceptor;
#[derive(Debug)]
pub struct HttpProxy {
    pub port: i32,
    pub channel: mpsc::Receiver<()>,
    pub mapping_key: String,
    pub shared_config: Arc<Mutex<AppConfig>>,
}

impl HttpProxy {
    pub async fn start_http_server(&mut self) -> Result<(), AppError> {
        let port_clone = self.port;
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        let client = HttpClients::new();
        let mapping_key_clone1 = self.mapping_key.clone();
        let reveiver = &mut self.channel;

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| AppError(e.to_string()))?;
        info!("Listening on http://{}", addr);
        loop {
            tokio::select! {
               Ok((stream,addr))= listener.accept()=>{
                let client_cloned = client.clone();
                let mapping_key2 = mapping_key_clone1.clone();
                let shared_app_config=self.shared_config.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);

                    if let Err(err) = http1::Builder::new()
                    .preserve_header_case(true)
                    .title_case_headers(true)
                        .serve_connection(
                            io,
                            service_fn(move |req: Request<Incoming>| {
                                let req = req.map(|item| {
                                    item.map_err(|_| -> Infallible { unreachable!() }).boxed()
                                });
                                proxy_adapter(shared_app_config.clone(),client_cloned.clone(), req, mapping_key2.clone(), addr)
                            }),
                        )
                        .await
                    {
                        error!("Error serving connection: {:?}", err);
                    }
                });
                },
                _ = reveiver.recv() => {
                    info!("http server stoped");
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
    ) -> Result<(), AppError> {
        let port_clone = self.port;
        let addr = SocketAddr::from(([0, 0, 0, 0], port_clone as u16));
        let client = HttpClients::new();
        let mapping_key_clone1 = self.mapping_key.clone();

        let mut cer_reader = BufReader::new(pem_str.as_bytes());
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
        let reveiver = &mut self.channel;

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| AppError(e.to_string()))?;
        info!("Listening on http://{}", addr);
        loop {
            tokio::select! {
                    Ok((tcp_stream,addr))= listener.accept()=>{
                let tls_acceptor = tls_acceptor.clone();

                let client = client.clone();
                let mapping_key2 = mapping_key_clone1.clone();
                let shared_app_config=self.shared_config.clone();

                tokio::spawn(async move {
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
                            .map(|item| item.map_err(|_| -> Infallible { unreachable!() }).boxed());

                        proxy_adapter(shared_app_config.clone(),client.clone(), req, mapping_key2.clone(), addr)
                    });
                    if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                        error!("Error serving connection: {:?}", err);
                    }
                });
            },
                    _ = reveiver.recv() => {
                        info!("https server stoped");
                        break;
                    }
                }
        }

        Ok(())
    }
}
async fn proxy_adapter(
    shared_config: Arc<Mutex<AppConfig>>,
    client: HttpClients,
    req: Request<BoxBody<Bytes, Infallible>>,
    mapping_key: String,
    remote_addr: SocketAddr,
) -> Result<Response<BoxBody<Bytes, Infallible>>, Infallible> {
    let result =
        proxy_adapter_with_error(shared_config, client, req, mapping_key, remote_addr).await;
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
    shared_config: Arc<Mutex<AppConfig>>,
    client: HttpClients,
    req: Request<BoxBody<Bytes, Infallible>>,
    mapping_key: String,
    remote_addr: SocketAddr,
) -> Result<Response<BoxBody<Bytes, Infallible>>, AppError> {
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
        shared_config,
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
            .map_err(|_| AppError(String::from("Can not get bytes from body")))?
            .to_bytes();
        let response_str =
            String::from_utf8(response_bytes.to_vec()).map_err(|e| AppError(e.to_string()))?;
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
    shared_config: Arc<Mutex<AppConfig>>,
    client: HttpClients,
    mut req: Request<BoxBody<Bytes, Infallible>>,
    mapping_key: String,
    remote_addr: SocketAddr,
    check_trait: impl CheckTrait,
) -> Result<Response<BoxBody<Bytes, Infallible>>, AppError> {
    debug!("req: {:?}", req);
    let inbound_headers = req.headers().clone();
    let uri = req.uri().clone();
    let check_result = check_trait
        .check_before_request(
            shared_config,
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
        }
        *req.uri_mut() = request_path
            .parse()
            .map_err(|err: InvalidUri| AppError(err.to_string()))?;
        let request_future = if request_path.contains("https") {
            client.request_https(req, DEFAULT_HTTP_TIMEOUT)
        } else {
            client.request_http(req, DEFAULT_HTTP_TIMEOUT)
        };
        let response_result = match request_future.await {
            Ok(response) => response.map_err(|e| AppError(String::from(e.to_string()))),
            _ => {
                return Err(AppError(format!(
                    "Request time out,the uri is {}",
                    request_path
                )))
            }
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
            // let anomaly_detection_status_lock =
            //     temporary_base_route.anomaly_detection_status.read().await;
            // let consecutive_5xx = anomaly_detection_status_lock.consecutive_5xx;
            // if is_5xx || consecutive_5xx > 0 {
            //     if let Err(err) = trigger_anomaly_detection(
            //         anomaly_detection,
            //         route.liveness_status.clone(),
            //         base_route,
            //         is_5xx,
            //         liveness_config,
            //     )
            //     .await
            //     {
            //         error!("{}", err);
            //     }
            // }
        }
        let res = response_result?
            .map(|b| b.boxed())
            .map(|item| item.map_err(|_| -> Infallible { unreachable!() }).boxed());
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
) -> Result<(), AppError> {
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
) -> Result<Response<BoxBody<Bytes, Infallible>>, AppError> {
    let static_ = Static::new(Path::new(base_route.endpoint.as_str()));
    let current_res = static_.clone().serve(req).await;
    if current_res.is_ok() {
        let res = current_res.unwrap();
        if res.status() == StatusCode::NOT_FOUND {
            let mut request: Request<()> = Request::default();
            if base_route.try_file.is_none() {
                return Err(AppError(String::from("Please config the try_file!")));
            }
            *request.uri_mut() = base_route.try_file.unwrap().parse().unwrap();
            return static_
                .clone()
                .serve(request)
                .await
                .map(|item| {
                    item.map(|body| {
                        body.boxed()
                            .map_err(|_| -> Infallible { unreachable!() })
                            .boxed()
                    })
                })
                .map_err(|e| AppError(e.to_string()));
        } else {
            return Ok(res.map(|body| {
                body.boxed()
                    .map_err(|_| -> Infallible { unreachable!() })
                    .boxed()
            }));
        }
    }
    let mut request: Request<()> = Request::default();
    if base_route.try_file.is_none() {
        return Err(AppError(String::from("Please config the try_file!")));
    }
    *request.uri_mut() = base_route.try_file.unwrap().parse().unwrap();
    static_
        .clone()
        .serve(request)
        .await
        .map(|item| {
            item.map(|body| {
                body.boxed()
                    .map_err(|_| -> Infallible { unreachable!() })
                    .boxed()
            })
        })
        .map_err(|e| AppError(e.to_string()))
}
