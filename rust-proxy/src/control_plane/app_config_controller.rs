use super::responder::ApiError;
use crate::configuration_service::app_config_servive::GLOBAL_APP_CONFIG;
use crate::vojo::app_config::ApiService;
use crate::vojo::app_config::AppConfig;
use crate::vojo::vojo::BaseResponse;
use rocket::route::Route;
use rocket::serde::json::Json;
#[get("/appConfig", format = "json")]
async fn get_app_config() -> Result<Json<BaseResponse<AppConfig>>, ApiError> {
    let app_config = GLOBAL_APP_CONFIG.read().await;
    Ok(Json(BaseResponse {
        response_code: 0,
        response_object: app_config.clone(),
    }))
}

#[post("/appConfig", format = "json", data = "<api_services_json>")]
async fn set_app_config(
    api_services_json: Json<Vec<ApiService>>,
) -> Result<Json<BaseResponse<u32>>, ApiError> {
    let api_services = api_services_json.into_inner();
    let mut rw_global_lock = GLOBAL_APP_CONFIG.write().await;
    (*rw_global_lock).api_service_config = api_services.clone();

    Ok(Json(BaseResponse {
        response_code: 0,
        response_object: 0,
    }))
}
pub fn get_app_config_controllers() -> Vec<Route> {
    routes![get_app_config, set_app_config]
}
