use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct BaseHealthCheckParam {
    pub timeout: i32,
    pub interval: i32,
}
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct BaseAnomalyDetectionParam {
    pub ejection_second: u64,
}
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct HttpAnomalyDetectionParam {
    pub consecutive_5xx: i32,
    pub base_anomaly_detection_param: BaseAnomalyDetectionParam,
}
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnomalyDetectionType {
    Http(HttpAnomalyDetectionParam),
}
// impl HealthCheckType {
//     pub fn get_base_param(&self) -> BaseHealthCheckParam {
//         return match self {
//             HealthCheckType::HttpGet(http_param) => http_param.base_health_check_param.clone(),
//             HealthCheckType::Mysql(base_pram) => base_pram.clone(),
//             HealthCheckType::Redis(base_pram) => base_pram.clone(),
//         };
//     }
// }
