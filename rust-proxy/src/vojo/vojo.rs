use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
pub struct BaseResponse<T> {
    pub response_code: i32,
    pub response_object: T,
}
