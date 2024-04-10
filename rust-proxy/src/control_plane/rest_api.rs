use crate::configuration_service::app_config_service::start_proxy;
use crate::configuration_service::app_config_service::GLOBAL_APP_CONFIG;
use crate::constants::common_constants::DEFAULT_TEMPORARY_DIR;
use crate::control_plane::lets_encrypt::lets_encrypt_certificate;
use crate::vojo::app_config::ApiService;
use crate::vojo::app_config::Route;
use crate::vojo::app_config::ServiceType;

use crate::vojo::app_error::AppError;
use crate::vojo::base_response::BaseResponse;
use crate::vojo::route::BaseRoute;
use axum::response::IntoResponse;
use axum::routing::delete;
use axum::routing::{get, post, put};
use axum::Router;
use http::header;
use prometheus::{Encoder, TextEncoder};
use std::collections::HashMap;
use std::convert::Infallible;
use std::env;
use std::net::SocketAddr;
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use uuid::Uuid;
static INTERNAL_SERVER_ERROR: &str = "Internal Server Error";
#[derive(Debug)]
struct MethodError;
async fn get_app_config() -> Result<impl axum::response::IntoResponse, Infallible> {
    let app_config = GLOBAL_APP_CONFIG.lock().await;
    let cloned_config = app_config.clone();
    drop(app_config);
    let data = BaseResponse {
        response_code: 0,
        response_object: cloned_config,
    };
    let res = match serde_json::to_string(&data) {
        Ok(json) => (axum::http::StatusCode::OK, json),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("No route {}", INTERNAL_SERVER_ERROR),
        ),
    };
    Ok(res)
}
async fn get_prometheus_metrics() -> Result<impl axum::response::IntoResponse, Infallible> {
    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    let encoder = TextEncoder::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    Ok((
        axum::http::StatusCode::OK,
        String::from_utf8(buffer).unwrap_or(String::from("value")),
    ))
}
async fn post_app_config(
    axum::extract::Json(api_services_vistor): axum::extract::Json<ApiService>,
) -> Result<impl axum::response::IntoResponse, Infallible> {
    let t = match post_app_config_with_error(api_services_vistor).await {
        Ok(r) => r.into_response(),
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            err.to_string(),
        )
            .into_response(),
    };
    return Ok(t);
}
async fn post_app_config_with_error(
    mut api_service: ApiService,
) -> Result<impl axum::response::IntoResponse, AppError> {
    let current_type = api_service.service_config.server_type.clone();
    if current_type == ServiceType::Https || current_type == ServiceType::Http2Tls {
        validate_tls_config(
            api_service.service_config.cert_str.clone(),
            api_service.service_config.key_str.clone(),
        )?;
    }
    let uuid = Uuid::new_v4().to_string();
    let cloned_port = api_service.listen_port.clone();
    let (sender, receiver) = mpsc::channel::<()>(1);
    api_service.api_service_id = uuid.clone();
    api_service.sender = sender;
    let mut rw_global_lock = GLOBAL_APP_CONFIG.lock().await;
    rw_global_lock
        .api_service_config
        .insert(uuid.clone(), api_service);
    drop(rw_global_lock);

    tokio::spawn(async move {
        if let Err(err) = save_config_to_file().await {
            error!("Save file error,the error is {}!", err);
        }
        start_proxy(cloned_port, receiver, current_type, uuid).await;
    });
    let data = BaseResponse {
        response_code: 0,
        response_object: 0,
    };
    let json_str = serde_json::to_string(&data).unwrap();
    Ok((axum::http::StatusCode::OK, json_str))
}
async fn delete_route(
    axum::extract::Path(route_id): axum::extract::Path<String>,
) -> Result<impl axum::response::IntoResponse, Infallible> {
    let mut rw_global_lock = GLOBAL_APP_CONFIG.lock().await;
    // let mut api_services = vec![];
    // for mut api_service in rw_global_lock.clone().api_service_config {
    //     api_service
    //         .service_config
    //         .routes
    //         .retain(|route| route.route_id != route_id);
    //     if !api_service.service_config.routes.is_empty() {
    //         api_services.push(api_service);
    //     }
    // }
    // rw_global_lock.api_service_config = api_services;
    tokio::spawn(async {
        if let Err(err) = save_config_to_file().await {
            error!("Save file error,the error is {}!", err);
        }
    });

    let data = BaseResponse {
        response_code: 0,
        response_object: 0,
    };
    let json_str = serde_json::to_string(&data).unwrap();
    Ok((axum::http::StatusCode::OK, json_str))
}

async fn put_route(
    axum::extract::Json(route_vistor): axum::extract::Json<Route>,
) -> Result<impl axum::response::IntoResponse, Infallible> {
    match put_route_with_error(route_vistor).await {
        Ok(r) => Ok((axum::http::StatusCode::OK, r)),
        Err(e) => Ok((axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
async fn put_route_with_error(route_vistor: Route) -> Result<String, AppError> {
    let mut rw_global_lock = GLOBAL_APP_CONFIG.lock().await;

    // let old_route = rw_global_lock
    //     .api_service_config
    //     .iter_mut()
    //     .flat_map(|item| item.service_config.routes.clone())
    //     .find(|item| item.route_id == route_vistor.route_id)
    //     .ok_or(AppError(String::from(
    //         "Can not find the route by route id!",
    //     )))?;

    // let mut new_route = route_vistor;

    // let old_base_clusters = old_route.clone().route_cluster.get_all_route().await?;
    // let hashmap = old_base_clusters
    //     .iter()
    //     .map(|item| (item.endpoint.clone(), item.clone()))
    //     .collect::<HashMap<String, BaseRoute>>();
    // let mut new_routes = new_route.route_cluster.get_all_route().await?;
    // for new_base_route in new_routes.iter_mut() {
    //     if hashmap.clone().contains_key(&new_base_route.endpoint) {
    //         let old_base_route = hashmap.get(&new_base_route.endpoint).unwrap();
    //         let mut alive = new_base_route.is_alive.write().await;
    //         *alive = *old_base_route.is_alive.write().await;
    //         let mut anomaly_detection_status =
    //             new_base_route.anomaly_detection_status.write().await;
    //         *anomaly_detection_status = old_base_route
    //             .anomaly_detection_status
    //             .write()
    //             .await
    //             .clone();
    //     }
    // }
    // for api_service in rw_global_lock.api_service_config.iter_mut() {
    //     for route in api_service.service_config.routes.iter_mut() {
    //         if route.route_id == route_vistor.route_id {
    //             *route = new_route.clone();
    //         }
    //     }
    // }
    tokio::spawn(async {
        if let Err(err) = save_config_to_file().await {
            error!("Save file error,the error is {}!", err);
        }
    });
    let data = BaseResponse {
        response_code: 0,
        response_object: 0,
    };
    Ok(serde_json::to_string(&data).unwrap())
}
async fn save_config_to_file() -> Result<(), AppError> {
    let read_global_lock = GLOBAL_APP_CONFIG.lock().await;
    let data = read_global_lock.clone();
    drop(read_global_lock);
    let result: bool = Path::new(DEFAULT_TEMPORARY_DIR).is_dir();
    if !result {
        let path = env::current_dir().map_err(|e| AppError(e.to_string()))?;
        let absolute_path = path.join(DEFAULT_TEMPORARY_DIR);
        std::fs::create_dir_all(absolute_path).map_err(|e| AppError(e.to_string()))?;
    }

    let mut f = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open("temporary/new_silverwind_config.yml")
        .await
        .map_err(|e| AppError(e.to_string()))?;
    let api_service_str = serde_yaml::to_string(&data).map_err(|e| AppError(e.to_string()))?;
    f.write_all(api_service_str.as_bytes())
        .await
        .map_err(|e| AppError(e.to_string()))?;
    Ok(())
}
fn validate_tls_config(
    cert_pem_option: Option<String>,
    key_pem_option: Option<String>,
) -> Result<(), AppError> {
    if cert_pem_option.is_none() || key_pem_option.is_none() {
        return Err(AppError(String::from("Cert or key is none")));
    }
    let cert_pem = cert_pem_option.unwrap();
    let mut cer_reader = std::io::BufReader::new(cert_pem.as_bytes());
    let result_certs = rustls_pemfile::certs(&mut cer_reader).next();
    if result_certs.is_none() || result_certs.unwrap().is_err() {
        return Err(AppError(String::from("Can not parse the certs pem.")));
    }
    let key_pem = key_pem_option.unwrap();
    let key_pem_result = pkcs8::PrivateKeyDocument::from_pem(key_pem.as_str());
    if key_pem_result.is_err() {
        return Err(AppError(String::from("Can not parse the key pem.")));
    }
    Ok(())
}
pub fn get_router() -> Router {
    axum::Router::new()
        .route("/appConfig", get(get_app_config).post(post_app_config))
        .route("/metrics", get(get_prometheus_metrics))
        .route("/route/:id", delete(delete_route))
        .route("/route", put(put_route))
        .route("/letsEncryptCertificate", post(lets_encrypt_certificate))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
}
pub async fn start_control_plane(port: i32) -> Result<(), AppError> {
    let app = get_router();

    let addr = SocketAddr::from(([0, 0, 0, 0], port as u16));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| AppError(e.to_string()))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| AppError(e.to_string()))?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::{
        body::Body,
        http::{self, Request},
    };
    use http_body_util::BodyExt;
    use lazy_static::lazy_static;
    use serde_json::json;
    use std::env;
    use tokio::runtime::{Builder, Runtime};
    use tower::ServiceExt; // for `call`, `oneshot`, and `ready`
    lazy_static! {
        pub static ref TOKIO_RUNTIME: Runtime = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("my-custom-name")
            .thread_stack_size(3 * 1024 * 1024)
            .enable_all()
            .build()
            .unwrap();
    }
    #[test]
    fn test_api_get_response_ok() {
        TOKIO_RUNTIME.block_on(async {
            let res = get_app_config().await.unwrap();
            assert_eq!(res.into_response().status(), StatusCode::OK);
        })
    }
    #[tokio::test]
    async fn test_api_post_response_error() {
        let app = get_router();

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/appConfig")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!([1, 2, 3, 4])).unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    #[tokio::test]
    async fn test_api_post_response_ok() {
        let req = r#"
            {
                "listen_port": 4486,
                "service_config": {
                    "server_type": "Http",
                    "routes": [
                        {
                            "matcher": {
                                "prefix": "/get",
                                "prefix_rewrite": "ssss"
                            },
                            "route_cluster": {
                                "type": "RandomRoute",
                                "routes": [
                                    {
                                        "base_route": {
                                            "endpoint": "http://localhost:8000",
                                            "try_file": null
                                        }
                                    }
                                ]
                            }
                        }
                    ]
                }
            }
        "#;
        let app = get_router();

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/appConfig")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(req))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let base_response: BaseResponse<i32> = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(base_response.response_code, 0);
    }
    #[test]
    fn test_validate_tls_config_successfully() {
        let private_key_path = env::current_dir()
            .unwrap()
            .join("config")
            .join("test_key.pem");
        let private_key = std::fs::read_to_string(private_key_path).unwrap();

        let certificate_path = env::current_dir()
            .unwrap()
            .join("config")
            .join("test_cert.pem");
        let certificate = std::fs::read_to_string(certificate_path).unwrap();

        let validation_res = validate_tls_config(Some(certificate), Some(private_key));
        assert!(validation_res.is_ok());
    }
    #[test]
    fn test_validate_tls_config_error_with_private_key() {
        let certificate_path = env::current_dir()
            .unwrap()
            .join("config")
            .join("test_cert.pem");
        let certificate = std::fs::read_to_string(certificate_path).unwrap();

        let private_key = String::from("private key");
        let validation_res = validate_tls_config(Some(certificate), Some(private_key));
        assert!(validation_res.is_err());
    }
    #[test]
    fn test_validate_tls_config_error_with_certificate() {
        let private_key_path = env::current_dir()
            .unwrap()
            .join("config")
            .join("test_key.pem");
        let private_key = std::fs::read_to_string(private_key_path).unwrap();
        let certificate = String::from("test");

        let validation_res = validate_tls_config(Some(certificate), Some(private_key));
        assert!(validation_res.is_err());
    }

    #[tokio::test]
    async fn test_post_response_ok() {
        let body = r#"
            {
                "listen_port": 4486,
                "service_config": {
                    "server_type": "Http",
                    "routes": [
                        {
                            "matcher": {
                                "prefix": "/get",
                                "prefix_rewrite": "ssss"
                            },
                            "route_cluster": {
                                "type": "RandomRoute",
                                "routes": [
                                    {
                                        "base_route": {
                                            "endpoint": "http://localhost:8000",
                                            "try_file": null
                                        }
                                    }
                                ]
                            }
                        }
                    ]
                }
            }
        "#;

        let app = get_router();

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/appConfig")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
        let base_response: BaseResponse<i32> = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(base_response.response_code, 0);
    }
    #[tokio::test]
    async fn test_get_response_ok() {
        let app = get_router();

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::GET)
                    .uri("/appConfig")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    #[tokio::test]
    async fn test_put_route_ok() {
        let body = r#"{
            "route_id": "90c66439-5c87-4902-aebb-1c2c9443c154",
            "host_name": null,
            "matcher": {
                "prefix": "/",
                "prefix_rewrite": "ssss"
            },
            "allow_deny_list": null,
            "authentication": null,
            "anomaly_detection": null,
            "liveness_config": null,
            "health_check": null,
            "ratelimit": null,
            "route_cluster": {
                "type": "RandomRoute",
                "routes": [
                    {
                        "base_route": {
                            "endpoint": "http://127.0.0.1:10000",
                            "try_file": null,
                            "is_alive": null
                        }
                    }
                ]
            }
        }"#;

        let app = get_router();

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::PUT)
                    .uri("/route")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
    #[tokio::test]
    async fn test_delete_route_ok() {
        let app = get_router();

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::DELETE)
                    .uri("/route/90c66439-5c87-4902-aebb-1c2c9443c154")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
