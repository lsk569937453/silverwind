use std::any::Any;
use rocket::serde::json::{Json, Value, json};
use rocket::serde::{Serialize, Deserialize};
#[derive(Serialize, Deserialize)]
#[serde(crate = "rocket::serde")]
pub struct BaseResponse<T>{
    pub response_code : i32,
    pub response_object : T,
}