use crate::configuration_service::app_config_service::GLOBAL_APP_CONFIG;
use crate::proxy::http_proxy::GeneralError;
use crate::vojo::app_config::ApiService;
use crate::vojo::app_config::ServiceType;
use crate::vojo::vojo::BaseResponse;
use prometheus::{Encoder, TextEncoder};
use std::convert::Infallible;
use std::net::SocketAddr;
use warp::http::{Response, StatusCode};
use warp::Filter;
use warp::{reject, Rejection, Reply};
static INTERNAL_SERVER_ERROR: &str = "Internal Server Error";
#[derive(Debug)]
struct MethodError;
impl reject::Reject for MethodError {}
async fn get_app_config() -> Result<impl warp::Reply, Infallible> {
    let app_config = GLOBAL_APP_CONFIG.read().await;
    let data = BaseResponse {
        response_code: 0,
        response_object: app_config.clone(),
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
        .map_err(|e| GeneralError(anyhow!(e.to_string())))
        .unwrap())
}

async fn post_app_config(api_services: Vec<ApiService>) -> Result<impl warp::Reply, Infallible> {
    let validata_result = api_services
        .iter()
        .filter(|s| s.service_config.server_type == ServiceType::HTTPS)
        .map(|s| {
            return validate_tls_config(
                s.service_config.cert_str.clone(),
                s.service_config.key_str.clone(),
            );
        })
        .collect::<Result<Vec<()>, anyhow::Error>>();
    if let Err(err) = validata_result {
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(err.to_string())
            .unwrap());
    }
    let mut rw_global_lock = GLOBAL_APP_CONFIG.write().await;
    (*rw_global_lock).api_service_config = api_services.clone();
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
fn validate_tls_config(
    cert_pem_option: Option<String>,
    key_pem_option: Option<String>,
) -> Result<(), anyhow::Error> {
    if cert_pem_option.is_none() || key_pem_option.is_none() {
        return Err(anyhow!("Cert or key is none"));
    }
    let cert_pem = cert_pem_option.unwrap();
    let mut cer_reader = std::io::BufReader::new(cert_pem.as_bytes());
    let result_certs = rustls_pemfile::certs(&mut cer_reader);
    if result_certs.is_err() || result_certs.unwrap().len() == 0 {
        return Err(anyhow!("Can not parse the certs pem."));
    }
    let key_pem = key_pem_option.unwrap();
    let key_pem_result = pkcs8::PrivateKeyDocument::from_pem(key_pem.as_str());
    if key_pem_result.is_err() {
        return Err(anyhow!("Can not parse the key pem."));
    }
    Ok(())
}

fn json_body() -> impl Filter<Extract = (Vec<ApiService>,), Error = warp::Rejection> + Clone {
    warp::body::content_length_limit(1024 * 16).and(warp::body::json())
}
pub async fn handle_not_found(reject: Rejection) -> Result<impl Reply, Rejection> {
    if reject.is_not_found() {
        Ok(StatusCode::NOT_FOUND)
    } else {
        Err(reject)
    }
}
pub async fn handle_custom(reject: Rejection) -> Result<impl Reply, Rejection> {
    if reject.find::<MethodError>().is_some() {
        Ok(StatusCode::METHOD_NOT_ALLOWED)
    } else {
        Err(reject)
    }
}

pub async fn start_control_plane(port: i32) {
    let post_app_config = warp::post()
        .and(warp::path("appConfig"))
        .and(warp::path::end())
        .and(json_body())
        .and_then(post_app_config)
        .recover(handle_not_found);
    let get_app_config = warp::path("appConfig").and_then(get_app_config);
    let get_prometheus_metrics = warp::path("metrics").and_then(get_prometheus_metrics);

    let get_request = warp::get()
        .and(get_app_config.or(get_prometheus_metrics))
        .recover(handle_not_found);

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
        post_app_config
            .or(get_request)
            .with(cors)
            .with(log)
            .recover(handle_custom),
    )
    .run(addr)
    .await;
}
#[cfg(test)]
mod tests {
    use super::*;
    use http::StatusCode;
    use lazy_static::lazy_static;
    use std::env;
    use tokio::runtime::{Builder, Runtime};
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
        let req = r#"[
            {
                "listen_port": 4486,
                "service_config": {
                    "server_type": "HTTP",
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
        ]"#;
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
            let base_response: BaseResponse<i32> = serde_json::from_slice(&body_bytes).unwrap();
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
        assert_eq!(validation_res.is_ok(), true);
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
        assert_eq!(validation_res.is_err(), true);
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
        assert_eq!(validation_res.is_err(), true);
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
        let body = r#"[
            {
                "listen_port": 4486,
                "service_config": {
                    "server_type": "HTTP",
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
        ]"#;
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
            let base_response: BaseResponse<i32> = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(base_response.response_code, 0);
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
}
