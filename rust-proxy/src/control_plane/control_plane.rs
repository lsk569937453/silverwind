

use crate::vojo::vojo::BaseResponse;

use std::borrow::Cow;
use rocket::State;
use rocket::tokio::sync::Mutex;
use rocket::serde::json::{Json, Value, json};
use rocket::serde::{Serialize, Deserialize};
use crate::dao::insert_tuple_batch_with_default;
use crate::pool::pgpool::{self, DbConnection};
// The type to represent the ID of a message.
type Id = usize;

// We're going to store all of the messages here. No need for a DB.
type MessageList = Mutex<Vec<String>>;
type Messages<'r> = &'r State<MessageList>;

#[derive(Serialize, Deserialize)]
#[serde(crate = "rocket::serde")]
struct Message<'r> {
    id: Option<Id>,
    message: Cow<'r, str>
}
//curl -kv -X POST "http://127.0.0.1:3721/json"  -d '{"id":3,"message":"my_password"}'    -H 'Content-Type: application/json'
#[post("/proxy/create", format = "json", data = "<message>")]
async fn new(message: Json<Message<'_>>, list: Messages<'_>) -> Option<Json<BaseResponse<usize>>> {
    // let mut list = list.lock().await;
    // let id = list.len();
    // list.push(message.message.to_string());
    let mut connection:DbConnection= pgpool::CONNECTION_POOL.clone().get().unwrap();
    let insert_resut=insert_tuple_batch_with_default(&mut connection);
    let base_response= match insert_resut {
        Ok(len)=>Ok(BaseResponse{response_code:0,response_object:len}),
        Err(err)=>
        {
            error!("insert error, error is:{}",err);
            Err(err)
        },
    }.unwrap();
    Some(Json(base_response))
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
        rocket.mount("/", routes![new, get])
            .register("/", catchers![not_found])
            .manage(MessageList::new(vec![]))
    })
}