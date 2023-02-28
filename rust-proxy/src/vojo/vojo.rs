use rocket::serde::{Deserialize, Serialize};
use std::any::Any;
#[derive(Serialize, Deserialize)]
#[serde(crate = "rocket::serde")]
pub struct BaseResponse<T> {
    pub response_code: i32,
    pub response_object: T,
}
