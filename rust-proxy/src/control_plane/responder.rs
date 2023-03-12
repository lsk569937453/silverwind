use crate::vojo::vojo::BaseResponse;
use rocket::http::ContentType;
use rocket::http::Status;
use rocket::request::Request;
use rocket::response::{self, Responder, Response};

use std::io::Cursor;
#[derive(Debug, Clone)]
pub enum ApiError {
    Internal(String),

    _NotFound(String),

    _BadRequest(String),
}
impl ApiError {
    fn get_http_status(&self) -> Status {
        match self {
            ApiError::Internal(_) => Status::InternalServerError,
            ApiError::_NotFound(_) => Status::NotFound,
            _ => Status::BadRequest,
        }
    }
    fn to_string(&self) -> &String {
        match self {
            ApiError::Internal(s) => s,
            ApiError::_NotFound(s) => s,
            ApiError::_BadRequest(s) => s,
        }
    }
}
impl<'r> Responder<'r, 'static> for ApiError {
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'static> {
        let err_response = serde_json::to_string(&BaseResponse {
            response_code: -1,
            response_object: self.to_string(),
        })
        .unwrap();

        Response::build()
            .status(self.get_http_status())
            .header(ContentType::JSON)
            .sized_body(err_response.len(), Cursor::new(err_response))
            .ok()
    }
}
