use super::app_config_controller;
use rocket::serde::json::{json, Value};

#[catch(404)]
fn not_found() -> Value {
    json!({
        "status": "error",
        "reason": "Resource was not found."
    })
}

pub fn stage() -> rocket::fairing::AdHoc {
    let routes = [app_config_controller::get_app_config_controllers()].concat();
    rocket::fairing::AdHoc::on_ignite("JSON", |rocket| async {
        rocket
            .mount("/", routes)
            .register("/", catchers![not_found])
    })
}
