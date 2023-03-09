use crate::vojo::vojo::BaseResponse;

use crate::dao::insert_tuple_batch_with_default;
use crate::pool::db_connection_pool::{self, DbConnection};

use rocket::serde::json::{json, Json, Value};
use rocket::serde::{Deserialize, Serialize};
use rocket::tokio::sync::Mutex;
use rocket::State;
use std::borrow::Cow;

use super::app_config_controller;
use super::responder::ApiError;
type Id = usize;

type MessageList = Mutex<Vec<String>>;
type Messages<'r> = &'r State<MessageList>;

#[derive(Serialize, Deserialize)]
#[serde(crate = "rocket::serde")]
struct Message<'r> {
    id: Option<Id>,
    message: Cow<'r, str>,
}
#[post("/proxy/create", format = "json", data = "<_message>")]
async fn new(_message: Json<Message<'_>>) -> Result<Json<BaseResponse<usize>>, ApiError> {
    let mut connection: DbConnection = match db_connection_pool::get_connection() {
        Ok(conn) => conn,
        Err(err) => return Err(ApiError::Internal(err.to_string())),
    };
    let insert_resut = insert_tuple_batch_with_default(&mut connection);
    let base_response = match insert_resut {
        Ok(len) => BaseResponse {
            response_code: 0,
            response_object: len,
        },
        Err(err) => {
            error!("insert error, error is:{}", err);
            let err_message = format!("insert error, error is:{}", err);
            return Err(ApiError::BadRequest(err_message));
        }
    };
    Ok(Json(base_response))
}

#[get("/<id>", format = "json")]
async fn get(id: Id, list: Messages<'_>) -> Option<Json<Message<'_>>> {
    let list = list.lock().await;

    Some(Json(Message {
        id: Some(id),
        message: list.get(id)?.to_string().into(),
    }))
}

#[catch(404)]
fn not_found() -> Value {
    json!({
        "status": "error",
        "reason": "Resource was not found."
    })
}

pub fn stage() -> rocket::fairing::AdHoc {
    let routes = [
        routes![new, get],
        app_config_controller::get_app_config_controllers(),
    ]
    .concat();
    rocket::fairing::AdHoc::on_ignite("JSON", |rocket| async {
        rocket
            .mount("/", routes)
            .register("/", catchers![not_found])
            .manage(MessageList::new(vec![]))
    })
}
