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
