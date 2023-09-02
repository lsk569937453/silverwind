use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct BaseHealthCheckParam {
    pub timeout: i32,
    pub interval: i32,
}
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct HttpHealthCheckParam {
    pub base_health_check_param: BaseHealthCheckParam,
    pub path: String,
}
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HealthCheckType {
    HttpGet(HttpHealthCheckParam),
    Redis(BaseHealthCheckParam),
    Mysql(BaseHealthCheckParam),
}
impl HealthCheckType {
    pub fn get_base_param(&self) -> BaseHealthCheckParam {
        match self {
            HealthCheckType::HttpGet(http_param) => http_param.base_health_check_param.clone(),
            HealthCheckType::Mysql(base_pram) => base_pram.clone(),
            HealthCheckType::Redis(base_pram) => base_pram.clone(),
        }
    }
}
