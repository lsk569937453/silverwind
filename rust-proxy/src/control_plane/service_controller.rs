
use std::borrow::Cow;

use rocket::State;
use rocket::tokio::sync::Mutex;
use rocket::serde::json::{Json, Value, json};
use rocket::serde::{Serialize, Deserialize};

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
#[post("/", format = "json", data = "<message>")]
async fn new(message: Json<Message<'_>>, list: Messages<'_>) -> Value {
    let mut list = list.lock().await;
    let id = list.len();
    list.push(message.message.to_string());
    json!({ "status": "ok", "id": id })
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
        rocket.mount("/json", routes![new, get])
            .register("/json", catchers![not_found])
            .manage(MessageList::new(vec![]))
    })
}