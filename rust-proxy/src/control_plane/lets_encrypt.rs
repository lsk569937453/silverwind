use crate::vojo::base_response::BaseResponse;
use crate::vojo::lets_encrypt::LetsEntrypt;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use warp::http::{Response, StatusCode};
use warp::Filter;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
struct LetsEncryptResponse {
    key_perm: String,
    certificate_perm: String,
}
async fn lets_encrypt_certificate(
    lets_encrypt_object: LetsEntrypt,
) -> Result<impl warp::Reply, Infallible> {
    let request_result = lets_encrypt_object.start_request().await;
    if let Err(err) = request_result {
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(err.to_string())
            .unwrap());
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
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(json_str)
        .unwrap())
}
fn json_body() -> impl Filter<Extract = (LetsEntrypt,), Error = warp::Rejection> + Clone {
    warp::body::content_length_limit(1024 * 16).and(warp::body::json())
}
pub fn path() -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    warp::path("letsEncryptCertificate")
        .and(warp::path::end())
        .and(json_body())
        .and_then(lets_encrypt_certificate)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lets_encrypt_certificate_error1() {
        let lets_entrypt = LetsEntrypt::_new(
            String::from("lsk@gmail.com"),
            String::from("www.silverwind.top"),
        );
        let json = serde_json::to_string(&lets_entrypt).unwrap();
        let post_app_config = warp::post()
            .and(warp::path("letsEncryptCertificate"))
            .and(warp::path::end())
            .and(json_body())
            .and_then(lets_encrypt_certificate);
        let res = warp::test::request()
            .method("POST")
            .body(json)
            .reply(&post_app_config)
            .await;

        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
    #[tokio::test]
    async fn test_lets_encrypt_certificate_error2() {
        let lets_entrypt = LetsEntrypt::_new(
            String::from("lsk@gmail.com"),
            String::from("www.silverwind.top"),
        );
        let json = serde_json::to_string(&lets_entrypt).unwrap();
        let post_app_config = warp::post()
            .and(warp::path("letsEncryptCertificate"))
            .and(warp::path::end())
            .and(json_body())
            .and_then(lets_encrypt_certificate);
        let res = warp::test::request()
            .path("/letsEncryptCertificate")
            .method("POST")
            .body(json)
            .reply(&post_app_config)
            .await;

        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
