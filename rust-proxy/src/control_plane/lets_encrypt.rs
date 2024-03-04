use crate::vojo::base_response::BaseResponse;
use crate::vojo::lets_encrypt::LetsEntrypt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::Infallible;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
struct LetsEncryptResponse {
    key_perm: String,
    certificate_perm: String,
}
pub async fn lets_encrypt_certificate(
    lets_encrypt_object: LetsEntrypt,
) -> Result<impl axum::response::IntoResponse, Infallible> {
    let request_result = lets_encrypt_object.start_request().await;
    if let Err(err) = request_result {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            err.to_string(),
        );
    }
    let certificate = request_result.unwrap();
    let response = LetsEncryptResponse {
        key_perm: String::from(certificate.private_key()),
        certificate_perm: String::from(certificate.certificate()),
    };
    let data = BaseResponse {
        response_code: 0,
        response_object: response,
    };
    let json_str = serde_json::to_string(&data).unwrap();

    Ok(json!(json_str).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_plane::rest_api::get_router;
    use axum::http::StatusCode;
    use axum::{
        body::Body,
        extract::connect_info::MockConnectInfo,
        http::{self, Request},
    };
    use lazy_static::lazy_static;
    use std::env;
    #[tokio::test]
    async fn test_lets_encrypt_certificate_error1() {
        let lets_entrypt = LetsEntrypt::_new(
            String::from("lsk@gmail.com"),
            String::from("www.silverwind.top"),
        );
        let json = serde_json::to_string(&lets_entrypt).unwrap();
        let app = get_router();

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/letsEncryptCertificate")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(json)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    #[tokio::test]
    async fn test_lets_encrypt_certificate_error2() {
        let lets_entrypt = LetsEntrypt::_new(
            String::from("lsk@gmail.com"),
            String::from("www.silverwind.top"),
        );
        let json = serde_json::to_string(&lets_entrypt).unwrap();

        let app = get_router();

        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::POST)
                    .uri("/letsEncryptCertificate")
                    .header(http::header::CONTENT_TYPE, "application/json")
                    .body(json)
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
