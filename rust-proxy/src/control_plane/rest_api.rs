use crate::configuration_service::app_config_service::GLOBAL_APP_CONFIG;
use crate::constants::common_constants::DEFAULT_TEMPORARY_DIR;
use crate::control_plane::lets_encrypt::path;
use crate::vojo::app_config::ApiService;
use crate::vojo::app_config::Route;
use crate::vojo::app_config::ServiceType;
use crate::vojo::app_config_vistor::from_api_service;
use crate::vojo::app_config_vistor::ApiServiceVistor;
use crate::vojo::app_config_vistor::AppConfigVistor;
use crate::vojo::app_config_vistor::RouteVistor;
use crate::vojo::app_error::AppError;
use crate::vojo::base_response::BaseResponse;
use crate::vojo::route::BaseRoute;
use prometheus::{Encoder, TextEncoder};
use std::collections::HashMap;
use std::convert::Infallible;
use std::env;
use std::net::SocketAddr;
use std::path::Path;
use tokio::io::AsyncWriteExt;
use warp::http::{Response, StatusCode};
use warp::Filter;
use warp::{reject, Rejection, Reply};
static INTERNAL_SERVER_ERROR: &str = "Internal Server Error";
#[derive(Debug)]
struct MethodError;
impl reject::Reject for MethodError {}
async fn get_app_config() -> Result<impl warp::Reply, Infallible> {
    let app_config = GLOBAL_APP_CONFIG.read().await;

    let app_config_vistor_result = AppConfigVistor::from(app_config.clone()).await;
    if app_config_vistor_result.is_err() {
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(INTERNAL_SERVER_ERROR.into())
            .unwrap());
    }
    let data = BaseResponse {
        response_code: 0,
        response_object: app_config_vistor_result.unwrap(),
    };
    let res = match serde_json::to_string(&data) {
        Ok(json) => Response::builder()
            .header("content-type", "application/json")
            .body(json)
            .unwrap(),
        Err(_) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(INTERNAL_SERVER_ERROR.into())
            .unwrap(),
    };
    Ok(res)
}
async fn get_prometheus_metrics() -> Result<impl warp::Reply, Infallible> {
    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    let encoder = TextEncoder::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(String::from_utf8(buffer).unwrap_or(String::from("value")))
        .map_err(|e| AppError(e.to_string()))
        .unwrap())
}
async fn post_app_config(
    api_services_vistor: ApiServiceVistor,
) -> Result<impl warp::Reply, Infallible> {
    match post_app_config_with_error(api_services_vistor).await {
        Ok(r) => Ok(r),
        Err(err) => Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(err.to_string())
            .unwrap()),
    }
}
async fn post_app_config_with_error(
    api_services_vistor: ApiServiceVistor,
) -> Result<Response<String>, AppError> {
    let current_type = api_services_vistor.service_config.server_type.clone();
    if current_type == ServiceType::Https || current_type == ServiceType::Http2Tls {
        validate_tls_config(
            api_services_vistor.service_config.cert_str.clone(),
            api_services_vistor.service_config.key_str.clone(),
        )?;
    }
    let api_service = ApiService::from(api_services_vistor).await?;
    let mut rw_global_lock = GLOBAL_APP_CONFIG.write().await;
    match rw_global_lock
        .api_service_config
        .iter_mut()
        .find(|item| item.listen_port == api_service.listen_port)
    {
        Some(data) => data.service_config.routes.push(
            api_service
                .service_config
                .routes
                .first()
                .ok_or(AppError(String::from("The route is empty!")))?
                .clone(),
        ),
        None => rw_global_lock.api_service_config.push(api_service),
    };
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
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(json_str)
        .unwrap())
}
async fn delete_route(route_id: String) -> Result<impl warp::Reply, Infallible> {
    let mut rw_global_lock = GLOBAL_APP_CONFIG.write().await;
    let mut api_services = vec![];
    for mut api_service in rw_global_lock.clone().api_service_config {
        api_service
            .service_config
            .routes
            .retain(|route| route.route_id != route_id);
        if !api_service.service_config.routes.is_empty() {
            api_services.push(api_service);
        }
    }
    rw_global_lock.api_service_config = api_services;
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
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(json_str)
        .unwrap())
}

async fn put_route(route_vistor: RouteVistor) -> Result<impl warp::Reply, Infallible> {
    match post_route_with_error(route_vistor).await {
        Ok(r) => Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(r)
            .unwrap()),
        Err(e) => Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header("content-type", "application/json")
            .body(e.to_string())
            .unwrap()),
    }
}
async fn post_route_with_error(route_vistor: RouteVistor) -> Result<String, AppError> {
    let mut rw_global_lock = GLOBAL_APP_CONFIG.write().await;

    let old_route = rw_global_lock
        .api_service_config
        .iter_mut()
        .flat_map(|item| item.service_config.routes.clone())
        .find(|item| item.route_id == route_vistor.route_id)
        .ok_or(AppError(String::from(
            "Can not find the route by route id!",
        )))?;

    let mut new_route = Route::from(route_vistor.clone()).await?;
    let mut new_liveness_status = new_route.liveness_status.write().await;
    *new_liveness_status = old_route.liveness_status.write().await.clone();

    let old_base_clusters = old_route.clone().route_cluster.get_all_route().await?;
    let hashmap = old_base_clusters
        .iter()
        .map(|item| (item.endpoint.clone(), item.clone()))
        .collect::<HashMap<String, BaseRoute>>();
    let mut new_routes = new_route.route_cluster.get_all_route().await?;
    for new_base_route in new_routes.iter_mut() {
        if hashmap.clone().contains_key(&new_base_route.endpoint) {
            let old_base_route = hashmap.get(&new_base_route.endpoint).unwrap();
            let mut alive = new_base_route.is_alive.write().await;
            *alive = *old_base_route.is_alive.write().await;
            let mut anomaly_detection_status =
                new_base_route.anomaly_detection_status.write().await;
            *anomaly_detection_status = old_base_route
                .anomaly_detection_status
                .write()
                .await
                .clone();
        }
    }
    for api_service in rw_global_lock.api_service_config.iter_mut() {
        for route in api_service.service_config.routes.iter_mut() {
            if route.route_id == route_vistor.route_id {
                *route = new_route.clone();
            }
        }
    }
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
    let read_global_lock = GLOBAL_APP_CONFIG.read().await;
    let data = read_global_lock.clone();
    drop(read_global_lock);
    let api_services_vistor = from_api_service(data.api_service_config.clone()).await?;
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
    let api_service_str =
        serde_yaml::to_string(&api_services_vistor).map_err(|e| AppError(e.to_string()))?;
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

fn json_body() -> impl Filter<Extract = (ApiServiceVistor,), Error = warp::Rejection> + Clone {
    warp::body::content_length_limit(1024 * 16).and(warp::body::json())
}
fn route_json_body() -> impl Filter<Extract = (RouteVistor,), Error = warp::Rejection> + Clone {
    warp::body::content_length_limit(1024 * 16).and(warp::body::json())
}

pub async fn handle_not_found(reject: Rejection) -> Result<impl Reply, Rejection> {
    if reject.is_not_found() {
        Ok(StatusCode::NOT_FOUND)
    } else {
        Err(reject)
    }
}
pub async fn _handle_custom(reject: Rejection) -> Result<impl Reply, Rejection> {
    if reject.find::<MethodError>().is_some() {
        Ok(StatusCode::METHOD_NOT_ALLOWED)
    } else {
        Err(reject)
    }
}

pub async fn start_control_plane(port: i32) {
    let post_app_config = warp::path("appConfig")
        .and(warp::path::end())
        .and(json_body())
        .and_then(post_app_config);
    let put_route = warp::path("route")
        .and(warp::path::end())
        .and(route_json_body())
        .and_then(put_route);
    let delete_route = warp::path("route")
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and_then(delete_route);
    let get_app_config = warp::path("appConfig").and_then(get_app_config);

    let get_prometheus_metrics = warp::path("metrics").and_then(get_prometheus_metrics);

    let get_request = warp::get().and(get_app_config.or(get_prometheus_metrics));
    let post_request = warp::post().and(path().or(post_app_config));
    let put_request = warp::put().and(put_route);
    let delete_request = warp::delete().and(delete_route);

    // let put_request = warp::put().and(path()).recover(handle_not_found);

    let log = warp::log("dashbaord-svc");

    let addr = SocketAddr::from(([0, 0, 0, 0], port as u16));

    let cors = warp::cors()
        .allow_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS", "HEAD"])
        .allow_credentials(true)
        .allow_headers(vec![
            "access-control-allow-methods",
            "access-control-allow-origin",
            "useragent",
            "content-type",
            "x-custom-header",
        ])
        .allow_any_origin();
    warp::serve(
        post_request
            .or(get_request)
            .or(put_request)
            .or(delete_request)
            .with(cors)
            .with(log)
            .recover(handle_not_found),
    )
    .run(addr)
    .await;
}
#[cfg(test)]
mod tests {
    use super::*;
    use lazy_static::lazy_static;
    use std::env;
    use tokio::runtime::{Builder, Runtime};
    use warp::http::StatusCode;
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
    #[test]
    fn test_api_post_response_error() {
        TOKIO_RUNTIME.block_on(async {
            let post_app_config = warp::post()
                .and(warp::path("appConfig"))
                .and(warp::path::end())
                .and(json_body())
                .and_then(post_app_config)
                .recover(handle_not_found);
            let res = warp::test::request()
                .method("POST")
                .body(String::from("some string"))
                .reply(&post_app_config)
                .await;

            assert_eq!(res.status(), StatusCode::NOT_FOUND);
        })
    }
    #[test]
    fn test_api_post_response_ok() {
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
        TOKIO_RUNTIME.block_on(async {
            let post_app_config = warp::post()
                .and(warp::path("appConfig"))
                .and(warp::path::end())
                .and(json_body())
                .and_then(post_app_config)
                .recover(handle_not_found);
            let res = warp::test::request()
                .method("POST")
                .path("/appConfig")
                .body(req)
                // .json(&true)
                .reply(&post_app_config)
                .await;

            assert_eq!(res.status(), StatusCode::OK);
            let body_bytes = res.body();
            let base_response: BaseResponse<i32> = serde_json::from_slice(body_bytes).unwrap();
            assert_eq!(base_response.response_code, 0);
        })
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
    #[test]
    fn test_response_not_found() {
        TOKIO_RUNTIME.block_on(async {
            let post_app_config = warp::post()
                .and(warp::path("appConfig"))
                .and(warp::path::end())
                .and(json_body())
                .and_then(post_app_config)
                .recover(handle_not_found);
            let res = warp::test::request()
                .method("POST")
                .reply(&post_app_config)
                .await;
            assert_eq!(res.status(), StatusCode::NOT_FOUND);
        })
    }
    #[test]
    fn test_post_response_ok() {
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
        TOKIO_RUNTIME.block_on(async {
            let post_app_config = warp::post()
                .and(warp::path("appConfig"))
                .and(warp::path::end())
                .and(json_body())
                .and_then(post_app_config)
                .recover(handle_not_found);
            let res = warp::test::request()
                .method("POST")
                .path("/appConfig")
                .body(body)
                .reply(&post_app_config)
                .await;
            assert_eq!(res.status(), StatusCode::OK);
            let body_bytes = res.body();
            let base_response: BaseResponse<i32> = serde_json::from_slice(body_bytes).unwrap();
            assert_eq!(base_response.response_code, 0);
            assert_eq!(base_response.response_object, 0);
        })
    }
    #[test]
    fn test_get_response_ok() {
        TOKIO_RUNTIME.block_on(async {
            let get_app_config = warp::get()
                .and(warp::path("appConfig"))
                .and(warp::path::end())
                .and_then(get_app_config)
                .recover(handle_not_found);
            let res = warp::test::request()
                .method("GET")
                .path("/appConfig")
                .reply(&get_app_config)
                .await;
            assert_eq!(res.status(), StatusCode::OK);
        })
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

        let put_route = warp::path("route")
            .and(warp::path::end())
            .and(route_json_body())
            .and_then(put_route);
        let res = warp::test::request()
            .method("PUT")
            .path("/route")
            .body(body)
            .reply(&put_route)
            .await;
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        // let body_bytes = res.body();
        // let base_response: BaseResponse<i32> = serde_json::from_slice(body_bytes).unwrap();
        // assert_eq!(base_response.response_code, 0);
        // assert_eq!(base_response.response_object, 0);
    }
    #[tokio::test]
    async fn test_delete_route_ok() {
        let delete_route = warp::path("route")
            .and(warp::path::param::<String>())
            .and(warp::path::end())
            .and_then(delete_route);
        let res = warp::test::request()
            .method("DELETE")
            .path("/route/90c66439-5c87-4902-aebb-1c2c9443c154")
            .body("foo=bar&baz=quux")
            .reply(&delete_route)
            .await;
        assert_eq!(res.status(), StatusCode::OK);
        // let body_bytes = res.body();
        // let base_response: BaseResponse<i32> = serde_json::from_slice(body_bytes).unwrap();
        // assert_eq!(base_response.response_code, 0);
        // assert_eq!(base_response.response_object, 0);
    }
}
