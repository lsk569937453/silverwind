use crate::configuration_service::app_config_service::GLOBAL_APP_CONFIG;
use crate::vojo::app_config::ApiService;
use crate::vojo::app_config::ServiceType;
use crate::vojo::vojo::BaseResponse;
use hyper::service::{make_service_fn, service_fn};
use hyper::{header, Body, Method, Request, Response, Server, StatusCode};

use crate::proxy::http_proxy::GeneralError;
static NOTFOUND: &[u8] = b"Not Found";
static INTERNAL_SERVER_ERROR: &[u8] = b"Internal Server Error";

async fn get_app_config() -> Result<Response<Body>, GeneralError> {
    let app_config = GLOBAL_APP_CONFIG.read().await;
    let data = BaseResponse {
        response_code: 0,
        response_object: app_config.clone(),
    };
    let res = match serde_json::to_string(&data) {
        Ok(json) => Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json))
            .unwrap(),
        Err(_) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(INTERNAL_SERVER_ERROR.into())
            .unwrap(),
    };
    Ok(res)
}

async fn post_app_config(req: Request<Body>) -> Result<Response<Body>, GeneralError> {
    let byte_stream = hyper::body::to_bytes(req)
        .await
        .map_err(|err| GeneralError(anyhow!(err.to_string())))?;
    let api_services: Vec<ApiService> = serde_json::from_slice(&byte_stream)
        .map_err(|err| GeneralError(anyhow!(err.to_string())))?;

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
    if validata_result.is_err() {
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(INTERNAL_SERVER_ERROR.into())
            .unwrap());
    }
    let mut rw_global_lock = GLOBAL_APP_CONFIG.write().await;
    (*rw_global_lock).api_service_config = api_services.clone();
    let data = BaseResponse {
        response_code: 0,
        response_object: 0,
    };
    let json_str = serde_json::to_string(&data).unwrap();

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json_str))
        .map_err(|e| GeneralError(anyhow!(e.to_string())))
}
pub async fn response(req: Request<Body>) -> Result<Response<Body>, GeneralError> {
    let response_result = match (req.method(), req.uri().path()) {
        (&Method::POST, "/appConfig") => post_app_config(req).await,
        (&Method::GET, "/appConfig") => get_app_config().await,
        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(NOTFOUND.into())
            .unwrap()),
    };
    match response_result {
        Ok(r) => Ok(r),
        Err(err) => {
            let error_response = BaseResponse {
                response_code: -1,
                response_object: err.to_string(),
            };

            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(serde_json::to_string(&error_response).unwrap()))
                .unwrap())
        }
    }
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
pub async fn start_control_plane(port: i32) -> Result<(), hyper::Error> {
    let addr = format!("127.0.0.1:{}", port).parse().unwrap();
    let new_service = make_service_fn(move |_| async {
        Ok::<_, GeneralError>(service_fn(move |req| response(req)))
    });
    let server = Server::bind(&addr).serve(new_service);
    println!("Listening on http://{}", addr);
    server.await
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vojo::app_config::AppConfig;
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
            assert_eq!(res.status(), StatusCode::OK);
        })
    }
    #[test]
    fn test_api_post_response_error() {
        TOKIO_RUNTIME.block_on(async {
            let request = Request::builder().body(Body::from("some string")).unwrap();
            let res = post_app_config(request).await;
            assert_eq!(res.is_err(), true);
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
            let request = Request::builder().body(Body::from(req)).unwrap();
            let res = post_app_config(request).await;
            assert_eq!(res.is_ok(), true);
            let response = res.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
            let base_response: BaseResponse<i32> = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(base_response.response_code, 0);
        })
    }
    #[test]
    fn test_validate_tls_config_successfully() {
        let private_key_path = env::current_dir()
            .unwrap()
            .join("config")
            .join("privkey.pem");
        let private_key = std::fs::read_to_string(private_key_path).unwrap();

        let certificate_path = env::current_dir()
            .unwrap()
            .join("config")
            .join("cacert.pem");
        let certificate = std::fs::read_to_string(certificate_path).unwrap();

        let validation_res = validate_tls_config(Some(certificate), Some(private_key));
        assert_eq!(validation_res.is_ok(), true);
    }
    #[test]
    fn test_validate_tls_config_error_with_private_key() {
        let certificate_path = env::current_dir()
            .unwrap()
            .join("config")
            .join("cacert.pem");
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
            .join("privkey.pem");
        let private_key = std::fs::read_to_string(private_key_path).unwrap();
        let certificate = String::from("test");

        let validation_res = validate_tls_config(Some(certificate), Some(private_key));
        assert_eq!(validation_res.is_err(), true);
    }
    #[test]
    fn test_response_not_found() {
        TOKIO_RUNTIME.block_on(async {
            let request = Request::builder().body(Body::empty()).unwrap();
            let res = response(request).await;
            assert_eq!(res.is_ok(), true);
            let response = res.unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
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
            let request = Request::builder()
                .uri("http://localhost:8870/appConfig")
                .method(Method::POST)
                .body(Body::from(body))
                .unwrap();
            let res = response(request).await;
            assert_eq!(res.is_ok(), true);
            let response = res.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
            let base_response: BaseResponse<i32> = serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(base_response.response_code, 0);
        })
    }
    #[test]
    fn test_get_response_ok() {
        TOKIO_RUNTIME.block_on(async {
            let request = Request::builder()
                .uri("http://localhost:8870/appConfig")
                .method(Method::GET)
                .body(Body::empty())
                .unwrap();
            let res = response(request).await;
            assert_eq!(res.is_ok(), true);
            let response = res.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body_bytes = hyper::body::to_bytes(response.into_body()).await.unwrap();
            let base_response: BaseResponse<AppConfig> =
                serde_json::from_slice(&body_bytes).unwrap();
            assert_eq!(base_response.response_code, 0);
        })
    }
}
