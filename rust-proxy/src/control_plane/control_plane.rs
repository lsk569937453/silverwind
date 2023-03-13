use crate::configuration_service::app_config_service::GLOBAL_APP_CONFIG;
use crate::vojo::app_config::ApiService;
use crate::vojo::app_config::ServiceType;
use crate::vojo::vojo::BaseResponse;
use hyper::service::{make_service_fn, service_fn};
use hyper::{header, Body, Method, Request, Response, Server, StatusCode};

type GenericError = Box<dyn std::error::Error + Send + Sync>;
type ResultX<T> = std::result::Result<T, GenericError>;

static NOTFOUND: &[u8] = b"Not Found";
static INTERNAL_SERVER_ERROR: &[u8] = b"Internal Server Error";

async fn api_get_response() -> ResultX<Response<Body>> {
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

async fn api_post_response(req: Request<Body>) -> ResultX<Response<Body>> {
    let byte_stream = hyper::body::to_bytes(req).await?;
    let api_services: Vec<ApiService> = serde_json::from_slice(&byte_stream).unwrap();

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

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json_str))?;
    Ok(response)
}
pub async fn response_examples(req: Request<Body>) -> ResultX<Response<Body>> {
    match (req.method(), req.uri().path()) {
        (&Method::POST, "/appConfig") => api_post_response(req).await,
        (&Method::GET, "/appConfig") => api_get_response().await,
        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(NOTFOUND.into())
            .unwrap()),
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
pub async fn start_control_plane() -> Result<(), hyper::Error> {
    let addr = "127.0.0.1:8870".parse().unwrap();
    let new_service = make_service_fn(move |_| async {
        Ok::<_, GenericError>(service_fn(move |req| response_examples(req)))
    });
    let server = Server::bind(&addr).serve(new_service);
    println!("Listening on http://{}", addr);
    server.await
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
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
}
