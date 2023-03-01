use crate::vojo::vojo::BaseResponse;

use crate::dao::insert_tuple_batch_with_default;
use crate::pool::pgpool::{self, DbConnection};

use rocket::serde::json::{json, Json, Value};
use rocket::serde::{Deserialize, Serialize};
use rocket::tokio::sync::Mutex;
use rocket::State;
use std::borrow::Cow;

use super::responder::ApiError;
// The type to represent the ID of a message.
type Id = usize;

// We're going to store all of the messages here. No need for a DB.
type MessageList = Mutex<Vec<String>>;
type Messages<'r> = &'r State<MessageList>;

#[derive(Serialize, Deserialize)]
#[serde(crate = "rocket::serde")]
struct Message<'r> {
    id: Option<Id>,
    message: Cow<'r, str>,
}
//curl -kv -X POST "http://127.0.0.1:3721/json"  -d '{"id":3,"message":"my_password"}'    -H 'Content-Type: application/json'
#[post("/proxy/create", format = "json", data = "<message>")]
async fn new(
    message: Json<Message<'_>>,
    list: Messages<'_>,
) -> Result<Json<BaseResponse<usize>>, ApiError> {
    // let mut list = list.lock().await;
    // let id = list.len();
    // list.push(message.message.to_string());
    info!("new start");

    let mut connection: DbConnection = match pgpool::get_connection() {
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

// curl -kv -X GET "http://127.0.0.1:3721/json/3"
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
    rocket::fairing::AdHoc::on_ignite("JSON", |rocket| async {
        rocket
            .mount("/", routes![new, get])
            .register("/", catchers![not_found])
            .manage(MessageList::new(vec![]))
    })
}
