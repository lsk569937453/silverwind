use crate::vojo::base_response::BaseResponse;
use crate::vojo::lets_encrypt::LetsEntrypt;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
struct LetsEncryptResponse {
    key_perm: String,
    certificate_perm: String,
}
pub async fn lets_encrypt_certificate(
    axum::extract::Json(lets_encrypt_object): axum::extract::Json<LetsEntrypt>,
) -> Result<impl axum::response::IntoResponse, Infallible> {
    let request_result = lets_encrypt_object.start_request().await;
    if let Err(err) = request_result {
        return Ok((
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            err.to_string(),
        ));
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

    Ok((axum::http::StatusCode::OK, json_str))
}
