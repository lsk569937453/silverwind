use crate::vojo::vojo::BaseResponse;
use diesel::result::Error as DieselError;
use rocket::http::ContentType;
use rocket::http::Status;
use rocket::request::Request;
use rocket::response::{self, Responder, Response};
use rocket::serde::json::{json, Json, Value};
use std::error::Error;

use std::io::Cursor;
// #[derive(Serialize)]
// pub struct ErrorResponse {
//     message: String,
// }
// pub struct ApiError(anyhow::Error);

#[derive(Debug, Clone)]
pub enum ApiError {
    Internal(String),

    NotFound(String),

    BadRequest(String),
}
impl ApiError {
    fn get_http_status(&self) -> Status {
        match self {
            ApiError::Internal(_) => Status::InternalServerError,
            ApiError::NotFound(_) => Status::NotFound,
            _ => Status::BadRequest,
        }
    }
}
impl<'r> Responder<'r, 'static> for ApiError {
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'static> {
        // serialize struct into json string
        let err_response = serde_json::to_string(&BaseResponse {
            response_code: 0,
            response_object: "ss",
        })
        .unwrap();

        Response::build()
            .status(self.get_http_status())
            .header(ContentType::JSON)
            .sized_body(err_response.len(), Cursor::new(err_response))
            .ok()
    }
}
