use crate::vojo::lets_encrypt::LetsEntrypt;
use hyper::Body;
use std::convert::Infallible;
use warp::http::{Response, StatusCode};
use warp::Filter;
async fn put_the_certificate_by_lets_encrypt(
    lets_encrypt_object: LetsEntrypt,
) -> Result<impl warp::Reply, Infallible> {
    let request_result = lets_encrypt_object.start_request().await;
    if let Err(err) = request_result {
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(err.to_string()))
            .unwrap());
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::empty())
        .unwrap())
}
fn json_body() -> impl Filter<Extract = (LetsEntrypt,), Error = warp::Rejection> + Clone {
    warp::body::content_length_limit(1024 * 16).and(warp::body::json())
}
pub fn path() -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    warp::path("putTheCertificate_")
        .and(json_body())
        .and_then(put_the_certificate_by_lets_encrypt)
}
