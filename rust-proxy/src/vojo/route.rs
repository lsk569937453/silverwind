use super::app_config::LivenessConfig;
use super::app_config::LivenessStatus;
use crate::vojo::anomaly_detection::HttpAnomalyDetectionParam;
use core::fmt::Debug;
use dyn_clone::DynClone;
use http::HeaderMap;
use http::HeaderValue;
use log::Level;
use rand::prelude::*;
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::any::Any;
use std::collections::HashSet;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::RwLock;
use tokio::time::{sleep, Duration};

#[typetag::serde(tag = "type")]
pub trait LoadbalancerStrategy: Sync + Send + DynClone {
    fn get_route(&mut self, headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, anyhow::Error>;

    fn get_all_route(&mut self) -> Result<Vec<BaseRoute>, anyhow::Error>;

    fn get_debug(&self) -> String {
        String::from("debug")
    }
    fn as_any(&self) -> &dyn Any;
}
dyn_clone::clone_trait_object!(LoadbalancerStrategy);

impl Debug for dyn LoadbalancerStrategy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let routes = self.get_debug().clone();
        write!(f, "{{{}}}", routes)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnomalyDetectionStatus {
    pub consecutive_5xx: i32,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BaseRoute {
    pub endpoint: String,
    pub try_file: Option<String>,
    #[serde(skip_serializing, skip_deserializing)]
    pub is_alive: Arc<RwLock<Option<bool>>>,
    #[serde(skip_serializing, skip_deserializing)]
    pub anomaly_detection_status: Arc<RwLock<AnomalyDetectionStatus>>,
}
impl BaseRoute {
    fn update_ok(
        &self,
        liveness_status_lock: Arc<RwLock<LivenessStatus>>,
    ) -> Result<(), anyhow::Error> {
        let mut is_alive_lock = self.is_alive.write().map_err(|e| anyhow!("{}", e))?;
        // let is_alive = is_alive_lock
        if is_alive_lock.is_none() {
            *is_alive_lock = Some(true);
        } else if !is_alive_lock.unwrap() {
            *is_alive_lock = Some(true);
            let mut liveness_status = liveness_status_lock.write().map_err(|e| anyhow!("{}", e))?;
            (*liveness_status).current_liveness_count += 1;
        }
        Ok(())
    }
    fn update_fail(
        &self,
        liveness_status_lock: Arc<RwLock<LivenessStatus>>,
    ) -> Result<(), anyhow::Error> {
        let mut is_alive_lock = self.is_alive.write().map_err(|e| anyhow!("{}", e))?;
        if is_alive_lock.is_none() || is_alive_lock.unwrap() {
            let mut liveness_status = liveness_status_lock.write().map_err(|e| anyhow!("{}", e))?;
            (*liveness_status).current_liveness_count -= 1;
            *is_alive_lock = Some(false);
        }
        Ok(())
    }
    pub fn update_health_check_status_with_ok(
        &self,
        liveness_status_lock: Arc<RwLock<LivenessStatus>>,
    ) -> Result<(), anyhow::Error> {
        let is_alive_lock = self.is_alive.read().map_err(|e| anyhow!("{}", e))?;
        let is_alive = is_alive_lock.unwrap_or(false);
        if !is_alive {
            drop(is_alive_lock);
            self.update_ok(liveness_status_lock)?;
        }
        Ok(())
    }
    pub fn update_health_check_status_with_fail(
        &self,
        liveness_status_lock: Arc<RwLock<LivenessStatus>>,
        liveness_config: LivenessConfig,
    ) -> Result<bool, anyhow::Error> {
        let liveness_status = liveness_status_lock.read().map_err(|e| anyhow!("{}", e))?;
        if liveness_status.current_liveness_count <= liveness_config.min_liveness_count {
            return Ok(false);
        }
        let is_alive_lock = self.is_alive.read().map_err(|e| anyhow!("{}", e))?;
        let is_alive = is_alive_lock.unwrap_or(true);
        if is_alive {
            drop(liveness_status);
            drop(is_alive_lock);
            self.update_fail(liveness_status_lock.clone())?;
            return Ok(true);
        }
        Ok(false)
    }
    pub fn trigger_http_anomaly_detection(
        &self,
        http_anomaly_detection_param: HttpAnomalyDetectionParam,
        liveness_status_lock: Arc<RwLock<LivenessStatus>>,
        is_5xx: bool,
        liveness_config: LivenessConfig,
    ) -> Result<(), anyhow::Error> {
        let consecutive_5xx_config = http_anomaly_detection_param.consecutive_5xx;
        let mut anomaly_detection_status = self
            .anomaly_detection_status
            .try_write()
            .map_err(|e| anyhow!("{}", e))?;
        if !is_5xx && (*anomaly_detection_status).consecutive_5xx > 0 {
            (*anomaly_detection_status).consecutive_5xx = 0;
            return Ok(());
        }

        if (*anomaly_detection_status).consecutive_5xx < consecutive_5xx_config - 1 {
            (*anomaly_detection_status).consecutive_5xx += 1;
        } else {
            drop(anomaly_detection_status);
            let update_result = self.update_health_check_status_with_fail(
                liveness_status_lock.clone(),
                liveness_config,
            )?;
            if update_result {
                let alive_lock = self.is_alive.clone();
                let ejection_second = http_anomaly_detection_param
                    .base_anomaly_detection_param
                    .ejection_second;
                let anomaly_detection_status_lock = self.anomaly_detection_status.clone();
                tokio::spawn(async move {
                    let res = BaseRoute::wait_for_alive(
                        alive_lock,
                        ejection_second,
                        liveness_status_lock,
                        anomaly_detection_status_lock,
                    )
                    .await;
                    if res.is_err() {
                        error!("Wait for alive error,the error is {}!", res.unwrap_err());
                    } else {
                        info!("Wait for alive successfully!");
                    }
                });
            }
        }
        Ok(())
    }

    pub async fn wait_for_alive(
        is_alive_lock: Arc<RwLock<Option<bool>>>,
        wait_second: u64,
        liveness_status_lock: Arc<RwLock<LivenessStatus>>,
        anomaly_detection_status_lock: Arc<RwLock<AnomalyDetectionStatus>>,
    ) -> Result<(), anyhow::Error> {
        sleep(Duration::from_secs(wait_second)).await;
        let mut is_alive_option = is_alive_lock.write().map_err(|e| anyhow!("{}", e))?;
        let mut liveness_status = liveness_status_lock.write().map_err(|e| anyhow!("{}", e))?;
        let mut anomaly_detection_status = anomaly_detection_status_lock
            .write()
            .map_err(|e| anyhow!("{}", e))?;
        *is_alive_option = Some(true);
        (*liveness_status).current_liveness_count += 1;
        (*anomaly_detection_status).consecutive_5xx = 0;
        Ok(())
    }
}
impl PartialEq for BaseRoute {
    fn eq(&self, other: &BaseRoute) -> bool {
        let status = self.endpoint.eq(&other.endpoint) && self.try_file.eq(&other.try_file);
        if !status {
            return !status;
        }
        let src_option = self.is_alive.read();
        let dst_option = other.is_alive.read();
        if src_option.is_err() || dst_option.is_err() {
            return false;
        }

        src_option.unwrap().eq(&dst_option.unwrap())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeightRoute {
    pub base_route: BaseRoute,
    #[serde(default = "default_weight")]
    pub weight: i32,
    #[serde(skip_serializing, skip_deserializing)]
    pub index: Arc<AtomicIsize>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplitSegment {
    pub split_by: String,
    pub split_list: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplitItem {
    pub header_key: String,
    pub header_value: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]

pub struct RegexMatch {
    pub value: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextMatch {
    pub value: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HeaderValueMappingType {
    REGEX(RegexMatch),
    TEXT(TextMatch),
    SPLIT(SplitSegment),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeaderRoute {
    pub base_route: BaseRoute,
    pub header_key: String,
    pub header_value_mapping_type: HeaderValueMappingType,
}

fn default_weight() -> i32 {
    100
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HeaderBasedRoute {
    pub routes: Vec<HeaderRoute>,
}

#[typetag::serde]
impl LoadbalancerStrategy for HeaderBasedRoute {
    fn get_all_route(&mut self) -> Result<Vec<BaseRoute>, anyhow::Error> {
        Ok(self
            .routes
            .iter()
            .map(|item| item.base_route.clone())
            .collect::<Vec<BaseRoute>>())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn get_route(&mut self, headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, anyhow::Error> {
        let mut alive_cluster: Vec<HeaderRoute> = vec![];
        for item in self.routes.clone() {
            let is_alve_result = item.base_route.is_alive.read();
            if is_alve_result.is_err() {
                continue;
            }
            let is_alive_option = is_alve_result.unwrap();
            let is_alive = is_alive_option.unwrap_or(true);
            if is_alive {
                alive_cluster.push(item.clone());
            }
        }
        for item in self.routes.iter() {
            let headers_contais_key = headers.contains_key(item.header_key.clone());
            if !headers_contais_key {
                continue;
            }
            let header_value = headers.get(item.header_key.clone()).unwrap();
            let header_value_str = header_value.to_str().unwrap();
            match item.clone().header_value_mapping_type {
                HeaderValueMappingType::REGEX(regex_str) => {
                    let re = Regex::new(&regex_str.value).unwrap();
                    let capture_option = re.captures(header_value_str);
                    if capture_option.is_none() {
                        continue;
                    } else {
                        return Ok(item.clone().base_route);
                    }
                }
                HeaderValueMappingType::TEXT(text_str) => {
                    if text_str.value == header_value_str {
                        return Ok(item.clone().base_route);
                    } else {
                        continue;
                    }
                }
                HeaderValueMappingType::SPLIT(split_segment) => {
                    let split_set: HashSet<_> =
                        header_value_str.split(&split_segment.split_by).collect();
                    if split_set.len() == 0 {
                        continue;
                    }
                    let mut flag = true;
                    for split_item in split_segment.split_list.iter() {
                        if !split_set.contains(split_item.clone().as_str()) {
                            flag = false;
                            break;
                        }
                    }
                    if flag {
                        return Ok(item.clone().base_route);
                    }
                }
            }
        }
        error!("Can not find the route!And siverWind has selected the first route!");

        let first = self.routes.first().unwrap().base_route.clone();
        Ok(first)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RandomBaseRoute {
    pub base_route: BaseRoute,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RandomRoute {
    pub routes: Vec<RandomBaseRoute>,
}
#[typetag::serde]
impl LoadbalancerStrategy for RandomRoute {
    fn get_all_route(&mut self) -> Result<Vec<BaseRoute>, anyhow::Error> {
        Ok(self
            .routes
            .iter()
            .map(|item| item.base_route.clone())
            .collect::<Vec<BaseRoute>>())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn get_route(&mut self, _headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, anyhow::Error> {
        let mut rng = thread_rng();
        let mut alive_cluster: Vec<BaseRoute> = vec![];
        for item in self.routes.clone() {
            let is_alve_result = item.base_route.is_alive.read();
            if is_alve_result.is_err() {
                continue;
            }
            let is_alive_option = is_alve_result.unwrap();
            let is_alive = is_alive_option.unwrap_or(true);
            if is_alive {
                alive_cluster.push(item.base_route.clone());
            }
        }
        let index = rng.gen_range(0..alive_cluster.len());
        let dst = alive_cluster[index].clone();
        Ok(dst)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PollBaseRoute {
    pub base_route: BaseRoute,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PollRoute {
    #[serde(skip_serializing, skip_deserializing)]
    pub current_index: Arc<AtomicUsize>,
    pub routes: Vec<PollBaseRoute>,
    // #[serde(skip_serializing, skip_deserializing)]
    // pub lock: Arc<Mutex<i32>>,
}
#[typetag::serde]
impl LoadbalancerStrategy for PollRoute {
    fn get_all_route(&mut self) -> Result<Vec<BaseRoute>, anyhow::Error> {
        Ok(self
            .routes
            .iter()
            .map(|item| item.base_route.clone())
            .collect::<Vec<BaseRoute>>())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn get_route(&mut self, _headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, anyhow::Error> {
        let mut alive_cluster: Vec<PollBaseRoute> = vec![];
        for item in self.routes.clone() {
            let is_alve_result = item.base_route.is_alive.read();
            if is_alve_result.is_err() {
                continue;
            }
            let is_alive_option = is_alve_result.unwrap();
            let is_alive = is_alive_option.unwrap_or(true);
            if is_alive {
                alive_cluster.push(item.clone());
            }
        }
        let older = self.current_index.fetch_add(1, Ordering::SeqCst);
        let len = alive_cluster.len();
        let current_index = older % len;
        let dst = self.routes[current_index].clone();
        if log_enabled!(Level::Debug) {
            debug!("PollRoute current index:{}", current_index as i32);
        }
        Ok(dst.base_route)
    }
}
#[derive(Debug, Clone, Default)]
pub struct WeightBasedRoute {
    // pub indexs: Arc<RwLock<Vec<AtomicIsize>>>,
    pub routes: Arc<RwLock<Vec<WeightRoute>>>,
}
impl<'de> Deserialize<'de> for WeightBasedRoute {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct VistorWeightBasedRoute {
            pub routes: Vec<WeightRoute>,
        }
        let data = VistorWeightBasedRoute::deserialize(deserializer).map(
            |VistorWeightBasedRoute { routes }| WeightBasedRoute {
                routes: Arc::new(RwLock::new(routes)),
            },
        )?;
        Ok(data)
    }
}
impl Serialize for WeightBasedRoute {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::Error;

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct VistorWeightBasedRoute {
            pub routes: Vec<WeightRoute>,
        }
        let vistor_weight_based_route = self
            .routes
            .read()
            .map_err(|e| Error::custom(e.to_string()))?;
        vistor_weight_based_route.serialize(serializer)
    }
}

impl WeightRoute {}
#[typetag::serde]
impl LoadbalancerStrategy for WeightBasedRoute {
    fn get_all_route(&mut self) -> Result<Vec<BaseRoute>, anyhow::Error> {
        let read_lock = self.routes.read().map_err(|e| anyhow!("{}", e))?;
        let array = read_lock
            .iter()
            .map(|item| item.base_route.clone())
            .collect::<Vec<BaseRoute>>();
        Ok(array)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn get_route(&mut self, _headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, anyhow::Error> {
        let cluster_read_lock = self.routes.read().map_err(|e| anyhow!("{}", e))?;
        for (pos, e) in cluster_read_lock.iter().enumerate() {
            let is_alive_option_lock = e.base_route.is_alive.read();
            if let Ok(is_alive_option) = is_alive_option_lock {
                let is_alive = is_alive_option.unwrap_or(true);
                if is_alive {
                    let old_value = e.index.fetch_sub(1, Ordering::SeqCst);
                    if old_value > 0 {
                        if log_enabled!(Level::Debug) {
                            debug!("WeightRoute current index:{}", pos as i32);
                        }
                        return Ok(e.base_route.clone());
                    }
                }
            }
        }
        drop(cluster_read_lock);

        let mut new_lock = self.routes.write().map_err(|e| anyhow!("{}", e))?;
        let index_is_alive = new_lock.iter().any(|f| {
            let tt = f.index.load(Ordering::SeqCst);
            tt.is_positive()
        });
        if !index_is_alive {
            (*new_lock).iter_mut().for_each(|mut weight_route| {
                weight_route.index = Arc::new(AtomicIsize::from(weight_route.weight as isize))
            });
        }
        drop(new_lock);
        let cluster_read_lock2 = self.routes.read().map_err(|e| anyhow!("{}", e))?;

        for (pos, e) in cluster_read_lock2.iter().enumerate() {
            let is_alive_option_lock = e.base_route.is_alive.read();
            if let Ok(is_alive_option) = is_alive_option_lock {
                let is_alive = is_alive_option.unwrap_or(true);
                if is_alive {
                    let old_value = e.index.fetch_sub(1, Ordering::SeqCst);
                    if old_value > 0 {
                        if log_enabled!(Level::Debug) {
                            debug!("WeightRoute current index:{}", pos as i32);
                        }
                        return Ok(e.base_route.clone());
                    }
                }
            }
        }
        Err(anyhow!("WeightRoute get route error"))
    }
}
#[cfg(test)]
mod tests {
    use std::vec;

    use super::*;
    use crate::vojo::{anomaly_detection::BaseAnomalyDetectionParam, app_config::ApiService};
    // fn get_routes() -> Vec<BaseRoute> {
    //     vec![
    //         BaseRoute {
    //             endpoint: String::from("http://localhost:4444"),
    //             try_file: None,
    //         },
    //         BaseRoute {
    //             endpoint: String::from("http://localhost:5555"),
    //             try_file: None,
    //         },
    //         BaseRoute {
    //             endpoint: String::from("http://localhost:5555"),
    //             try_file: None,
    //         },
    //     ]
    // }
    fn get_random_routes() -> Vec<RandomBaseRoute> {
        vec![
            RandomBaseRoute {
                base_route: {
                    BaseRoute {
                        endpoint: String::from("http://localhost:4444"),
                        try_file: None,
                        is_alive: Arc::new(RwLock::new(None)),
                        anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        })),
                    }
                },
            },
            RandomBaseRoute {
                base_route: {
                    BaseRoute {
                        endpoint: String::from("http://localhost:5555"),
                        try_file: None,
                        is_alive: Arc::new(RwLock::new(None)),
                        anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        })),
                    }
                },
            },
            RandomBaseRoute {
                base_route: {
                    BaseRoute {
                        endpoint: String::from("http://localhost:5555"),
                        try_file: None,
                        is_alive: Arc::new(RwLock::new(None)),
                        anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        })),
                    }
                },
            },
        ]
    }
    fn get_poll_routes() -> Vec<PollBaseRoute> {
        vec![
            PollBaseRoute {
                base_route: {
                    BaseRoute {
                        endpoint: String::from("http://localhost:4444"),
                        try_file: None,
                        is_alive: Arc::new(RwLock::new(None)),
                        anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        })),
                    }
                },
            },
            PollBaseRoute {
                base_route: {
                    BaseRoute {
                        endpoint: String::from("http://localhost:5555"),
                        try_file: None,
                        is_alive: Arc::new(RwLock::new(None)),
                        anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        })),
                    }
                },
            },
            PollBaseRoute {
                base_route: {
                    BaseRoute {
                        endpoint: String::from("http://localhost:5555"),
                        try_file: None,
                        is_alive: Arc::new(RwLock::new(None)),
                        anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        })),
                    }
                },
            },
        ]
    }
    fn get_weight_routes() -> Vec<WeightRoute> {
        vec![
            WeightRoute {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:4444"),
                    try_file: None,
                    is_alive: Arc::new(RwLock::new(None)),
                    anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    })),
                },
                weight: 100,
                index: Arc::new(AtomicIsize::new(0)),
            },
            WeightRoute {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:5555"),
                    anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    })),
                    try_file: None,
                    is_alive: Arc::new(RwLock::new(None)),
                },
                weight: 100,
                index: Arc::new(AtomicIsize::new(0)),
            },
            WeightRoute {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:6666"),
                    try_file: None,
                    is_alive: Arc::new(RwLock::new(None)),
                    anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    })),
                },
                weight: 100,
                index: Arc::new(AtomicIsize::new(0)),
            },
        ]
    }
    fn get_header_based_routes() -> Vec<HeaderRoute> {
        vec![
            HeaderRoute {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:4444"),
                    try_file: None,
                    is_alive: Arc::new(RwLock::new(None)),
                    anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    })),
                },
                header_key: String::from("x-client"),
                header_value_mapping_type: HeaderValueMappingType::REGEX(RegexMatch {
                    value: String::from("^100*"),
                }),
            },
            HeaderRoute {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:5555"),
                    try_file: None,
                    is_alive: Arc::new(RwLock::new(None)),
                    anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    })),
                },
                header_key: String::from("x-client"),
                header_value_mapping_type: HeaderValueMappingType::SPLIT(SplitSegment {
                    split_by: String::from(";"),
                    split_list: vec![
                        String::from("a=1"),
                        String::from("b=2"),
                        String::from("c:3"),
                    ],
                }),
            },
            HeaderRoute {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:7777"),
                    try_file: None,
                    is_alive: Arc::new(RwLock::new(None)),
                    anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    })),
                },
                header_key: String::from("x-client"),
                header_value_mapping_type: HeaderValueMappingType::SPLIT(SplitSegment {
                    split_by: String::from(","),
                    split_list: vec![
                        String::from("a:12"),
                        String::from("b:9"),
                        String::from("c=7"),
                    ],
                }),
            },
            HeaderRoute {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:8888"),
                    try_file: None,
                    is_alive: Arc::new(RwLock::new(None)),
                    anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    })),
                },
                header_key: String::from("x-client"),
                header_value_mapping_type: HeaderValueMappingType::TEXT(TextMatch {
                    value: String::from("google chrome"),
                }),
            },
        ]
    }

    #[test]
    fn test_max_value() {
        let atomic = AtomicUsize::new(0);
        let old_value = atomic.fetch_add(1, Ordering::SeqCst);
        println!("{}", old_value);
    }
    #[test]
    fn test_poll_route_successfully() {
        let routes = get_poll_routes();
        let mut poll_rate = PollRoute {
            current_index: Default::default(),
            routes: routes.clone(),
            // lock: Default::default(),
        };
        for i in 0..100 {
            let current_route = poll_rate.get_route(HeaderMap::new()).unwrap();
            assert_eq!(current_route, routes[i % routes.len()].base_route);
        }
    }
    #[test]
    fn test_random_route_successfully() {
        let routes = get_random_routes();
        let mut random_rate = RandomRoute {
            routes: routes.clone(),
        };
        for _ in 0..100 {
            random_rate.get_route(HeaderMap::new()).unwrap();
        }
    }
    #[test]
    fn test_weight_route_successfully() {
        let routes = get_weight_routes();
        let mut weight_route = WeightBasedRoute {
            routes: Arc::new(RwLock::new(routes.clone())),
        };
        for _ in 0..100 {
            let current_route = weight_route.get_route(HeaderMap::new()).unwrap();
            assert_eq!(current_route, routes[0].base_route);
        }
        for _ in 0..100 {
            let current_route = weight_route.get_route(HeaderMap::new()).unwrap();
            assert_eq!(current_route, routes[1].base_route);
        }
        for _ in 0..100 {
            let current_route = weight_route.get_route(HeaderMap::new()).unwrap();
            assert_eq!(current_route, routes[2].base_route);
        }
        for _ in 0..100 {
            let current_route = weight_route.get_route(HeaderMap::new()).unwrap();
            assert_eq!(current_route, routes[0].base_route);
        }
    }
    #[test]
    fn test_debug_trait() {
        let weight_route: Box<dyn LoadbalancerStrategy> = Box::new(WeightBasedRoute {
            routes: Default::default(),
        });
        assert_eq!(format!("{:?}", weight_route), "{debug}");
    }
    #[test]
    fn test_serde_default_weight() {
        let req = r#"[
            {
              "listen_port": 4486,
              "service_config": {
                "server_type": "HTTP",
                "cert_str": null,
                "key_str": null,
                "routes": [
                  {
                    "matcher": {
                      "prefix": "ss",
                      "prefix_rewrite": "ssss"
                    },
                    "allow_deny_list": null,
                    "route_cluster": {
                      "type": "WeightBasedRoute",
                      "routes": [
                        {
                          "base_route": {
                            "endpoint": "/",
                            "try_file": null
                          }
                        }
                      ]
                    }
                  }
                ]
              }
            }
          ]"#;
        let api_services: Vec<ApiService> = serde_json::from_slice(req.as_bytes()).unwrap();
        let first_api_service = api_services
            .first()
            .unwrap()
            .service_config
            .routes
            .first()
            .unwrap()
            .clone();
        let route = first_api_service.route_cluster.clone();
        let weight_based_route: &WeightBasedRoute =
            match route.as_any().downcast_ref::<WeightBasedRoute>() {
                Some(b) => b,
                None => panic!("&a isn't a B!"),
            };

        println!(
            "the len is {}",
            weight_based_route.routes.read().unwrap().len()
        );
        assert_eq!(
            weight_based_route
                .clone()
                .routes
                .read()
                .unwrap()
                .first()
                .unwrap()
                .weight,
            100
        )
    }

    #[test]
    fn test_header_based_route_as_any() {
        let req = r#"[
            {
              "listen_port": 4486,
              "service_config": {
                "server_type": "HTTP",
                "cert_str": null,
                "key_str": null,
                "routes": [
                  {
                    "matcher": {
                      "prefix": "ss",
                      "prefix_rewrite": "ssss"
                    },
                    "allow_deny_list": null,
                    "route_cluster": {
                      "type": "HeaderBasedRoute",
                      "routes": [
                        {
                          "base_route": {
                            "endpoint": "/",
                            "try_file": null
                          },
                          "header_key": "user-agent",
                          "header_value_mapping_type": {
                            "type": "REGEX",
                            "value": "^100$"
                          }
                        }
                      ]
                    }
                  }
                ]
              }
            }
          ]"#;
        let api_services: Vec<ApiService> = serde_json::from_slice(req.as_bytes()).unwrap();
        let first_api_service = api_services
            .first()
            .unwrap()
            .service_config
            .routes
            .first()
            .unwrap()
            .clone();
        let route = first_api_service.route_cluster;
        let header_based_route: &HeaderBasedRoute =
            match route.as_any().downcast_ref::<HeaderBasedRoute>() {
                Some(b) => b,
                None => panic!("&a isn't a B!"),
            };
        let regex_match = RegexMatch {
            value: String::from("^100$"),
        };
        assert_eq!(
            header_based_route
                .routes
                .first()
                .unwrap()
                .header_value_mapping_type,
            HeaderValueMappingType::REGEX(regex_match)
        )
    }
    #[test]
    fn test_random_route_as_any() {
        let req = r#"[
            {
              "listen_port": 4486,
              "service_config": {
                "server_type": "HTTP",
                "cert_str": null,
                "key_str": null,
                "routes": [
                  {
                    "matcher": {
                      "prefix": "ss",
                      "prefix_rewrite": "ssss"
                    },
                    "allow_deny_list": null,
                    "route_cluster": {
                      "type": "RandomRoute",
                      "routes": [
                        {
                            "base_route": {
                                "endpoint": "/",
                                "try_file": null
                            }
                        }
                      ]
                    }
                  }
                ]
              }
            }
          ]"#;
        let api_services: Vec<ApiService> = serde_json::from_slice(req.as_bytes()).unwrap();
        let first_api_service = api_services
            .first()
            .unwrap()
            .service_config
            .routes
            .first()
            .unwrap()
            .clone();
        let route = first_api_service.route_cluster;
        let header_based_route: &RandomRoute = match route.as_any().downcast_ref::<RandomRoute>() {
            Some(b) => b,
            None => panic!("&a isn't a B!"),
        };

        assert_eq!(
            header_based_route
                .routes
                .first()
                .unwrap()
                .base_route
                .endpoint,
            "/"
        );
    }
    #[test]
    fn test_poll_route_as_any() {
        let req = r#"[
            {
              "listen_port": 4486,
              "service_config": {
                "server_type": "HTTP",
                "cert_str": null,
                "key_str": null,
                "routes": [
                  {
                    "matcher": {
                      "prefix": "ss",
                      "prefix_rewrite": "ssss"
                    },
                    "allow_deny_list": null,
                    "route_cluster": {
                      "type": "PollRoute",
                      "routes": [
                        {
                            "base_route": {
                                "endpoint": "/",
                                "try_file": null
                            }
                        }
                      ]
                    }
                  }
                ]
              }
            }
          ]"#;
        let api_services: Vec<ApiService> = serde_json::from_slice(req.as_bytes()).unwrap();
        let first_api_service = api_services
            .first()
            .unwrap()
            .service_config
            .routes
            .first()
            .unwrap()
            .clone();
        let route = first_api_service.route_cluster;
        let header_based_route: &PollRoute = match route.as_any().downcast_ref::<PollRoute>() {
            Some(b) => b,
            None => panic!("&a isn't a B!"),
        };

        assert_eq!(
            header_based_route
                .routes
                .first()
                .unwrap()
                .base_route
                .endpoint,
            "/"
        );
    }
    #[test]
    fn test_header_based_route_successfully() {
        let routes = get_header_based_routes();
        let header_route = HeaderBasedRoute { routes: routes };
        let mut header_route: Box<dyn LoadbalancerStrategy> = Box::new(header_route);
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("x-client", "100zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap());
        let result1 = header_route.get_route(headermap1.clone());
        assert_eq!(result1.is_ok(), true);
        assert_eq!(result1.unwrap().endpoint, "http://localhost:4444");

        let mut headermap2 = HeaderMap::new();
        headermap2.insert("x-client", "a=1;b=2;c:3;d=4;f5=6667".parse().unwrap());
        let result2 = header_route.get_route(headermap2.clone());
        assert_eq!(result2.is_ok(), true);
        assert_eq!(result2.unwrap().endpoint, "http://localhost:5555");

        let mut headermap3 = HeaderMap::new();
        headermap3.insert("x-client", "a:12,b:9,c=7,d=4;f5=6667".parse().unwrap());
        let result3 = header_route.get_route(headermap3.clone());
        assert_eq!(result3.is_ok(), true);
        assert_eq!(result3.unwrap().endpoint, "http://localhost:7777");

        let mut headermap4 = HeaderMap::new();
        headermap4.insert("x-client", "google chrome".parse().unwrap());
        let result4 = header_route.get_route(headermap4.clone());
        assert_eq!(result4.is_ok(), true);
        assert_eq!(result4.unwrap().endpoint, "http://localhost:8888");
    }
    #[test]
    fn test_update_health_check_status_with_ok_success1() {
        let base_route = BaseRoute {
            endpoint: String::from("/"),
            try_file: None,
            is_alive: Arc::new(RwLock::new(None)),
            anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                consecutive_5xx: 0,
            })),
        };
        let liveness_status_lock = Arc::new(RwLock::new(LivenessStatus {
            current_liveness_count: 3,
        }));

        let result = base_route.update_health_check_status_with_ok(liveness_status_lock.clone());
        assert_eq!(result.is_ok(), true);
        let is_alive_option = base_route.is_alive.read().unwrap();
        assert_eq!(is_alive_option.is_some(), true);
        assert_eq!(is_alive_option.unwrap(), true);
        let liveness_status = liveness_status_lock.read().unwrap();
        assert_eq!(liveness_status.current_liveness_count, 3);
    }
    #[test]
    fn test_update_health_check_status_with_ok_success2() {
        let base_route = BaseRoute {
            endpoint: String::from("/"),
            try_file: None,
            is_alive: Arc::new(RwLock::new(Some(true))),
            anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                consecutive_5xx: 0,
            })),
        };
        let liveness_status_lock = Arc::new(RwLock::new(LivenessStatus {
            current_liveness_count: 3,
        }));

        let result = base_route.update_health_check_status_with_ok(liveness_status_lock.clone());
        assert_eq!(result.is_ok(), true);
        let is_alive_option = base_route.is_alive.read().unwrap();
        assert_eq!(is_alive_option.is_some(), true);
        assert_eq!(is_alive_option.unwrap(), true);
        let liveness_status = liveness_status_lock.read().unwrap();
        assert_eq!(liveness_status.current_liveness_count, 3);
    }
    #[test]
    fn test_update_health_check_status_with_ok_success3() {
        let base_route = BaseRoute {
            endpoint: String::from("/"),
            try_file: None,
            is_alive: Arc::new(RwLock::new(Some(false))),
            anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                consecutive_5xx: 0,
            })),
        };
        let liveness_status_lock = Arc::new(RwLock::new(LivenessStatus {
            current_liveness_count: 3,
        }));

        let result = base_route.update_health_check_status_with_ok(liveness_status_lock.clone());
        assert_eq!(result.is_ok(), true);
        let is_alive_option = base_route.is_alive.read().unwrap();
        assert_eq!(is_alive_option.is_some(), true);
        assert_eq!(is_alive_option.unwrap(), true);
        let liveness_status = liveness_status_lock.read().unwrap();
        assert_eq!(liveness_status.current_liveness_count, 4);
    }

    #[test]
    fn test_update_health_check_status_with_fail_success1() {
        let base_route = BaseRoute {
            endpoint: String::from("/"),
            try_file: None,
            is_alive: Arc::new(RwLock::new(None)),
            anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                consecutive_5xx: 0,
            })),
        };
        let liveness_status_lock = Arc::new(RwLock::new(LivenessStatus {
            current_liveness_count: 3,
        }));

        let result = base_route.update_health_check_status_with_fail(
            liveness_status_lock.clone(),
            LivenessConfig {
                min_liveness_count: 3,
            },
        );
        assert_eq!(result.is_ok(), true);
        let is_alive_option = base_route.is_alive.read().unwrap();
        assert_eq!(is_alive_option.is_none(), true);
    }
    #[test]
    fn test_update_health_check_status_with_fail_success2() {
        let base_route = BaseRoute {
            endpoint: String::from("/"),
            try_file: None,
            is_alive: Arc::new(RwLock::new(Some(true))),
            anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                consecutive_5xx: 0,
            })),
        };
        let liveness_status_lock = Arc::new(RwLock::new(LivenessStatus {
            current_liveness_count: 3,
        }));

        let result = base_route.update_health_check_status_with_fail(
            liveness_status_lock.clone(),
            LivenessConfig {
                min_liveness_count: 3,
            },
        );
        assert_eq!(result.is_ok(), true);
        //update fails
        assert_eq!(result.unwrap(), false);
        let is_alive_option = base_route.is_alive.read().unwrap();
        assert_eq!(is_alive_option.is_some(), true);
        assert_eq!(is_alive_option.unwrap(), true);
        let liveness_status = liveness_status_lock.read().unwrap();
        assert_eq!(liveness_status.current_liveness_count, 3);
    }
    #[test]
    fn test_update_health_check_status_with_fail_success3() {
        let base_route = BaseRoute {
            endpoint: String::from("/"),
            try_file: None,
            is_alive: Arc::new(RwLock::new(Some(false))),
            anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                consecutive_5xx: 0,
            })),
        };
        let liveness_status_lock = Arc::new(RwLock::new(LivenessStatus {
            current_liveness_count: 3,
        }));

        let result = base_route.update_health_check_status_with_fail(
            liveness_status_lock.clone(),
            LivenessConfig {
                min_liveness_count: 3,
            },
        );
        assert_eq!(result.is_ok(), true);
        let is_alive_option = base_route.is_alive.read().unwrap();
        assert_eq!(is_alive_option.is_some(), true);
        assert_eq!(is_alive_option.unwrap(), false);
        let liveness_status = liveness_status_lock.read().unwrap();
        assert_eq!(liveness_status.current_liveness_count, 3);
    }

    #[test]
    fn test_trigger_http_anomaly_detection_success1() {
        let base_route = BaseRoute {
            endpoint: String::from("/"),
            try_file: None,
            is_alive: Arc::new(RwLock::new(Some(false))),
            anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                consecutive_5xx: 0,
            })),
        };
        let liveness_status_lock = Arc::new(RwLock::new(LivenessStatus {
            current_liveness_count: 3,
        }));
        let http_anomaly_detection_param = HttpAnomalyDetectionParam {
            consecutive_5xx: 2,
            base_anomaly_detection_param: BaseAnomalyDetectionParam { ejection_second: 3 },
        };
        let result = base_route.trigger_http_anomaly_detection(
            http_anomaly_detection_param,
            liveness_status_lock.clone(),
            true,
            LivenessConfig {
                min_liveness_count: 3,
            },
        );
        assert_eq!(result.is_ok(), true);
        let anomaly_detection_status = base_route.anomaly_detection_status.read().unwrap();
        assert_eq!(anomaly_detection_status.consecutive_5xx, 1);
    }
    #[tokio::test]
    async fn test_trigger_http_anomaly_detection_success2() {
        let base_route = BaseRoute {
            endpoint: String::from("/"),
            try_file: None,
            is_alive: Arc::new(RwLock::new(Some(true))),
            anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                consecutive_5xx: 1,
            })),
        };
        let liveness_status_lock = Arc::new(RwLock::new(LivenessStatus {
            current_liveness_count: 4,
        }));
        let http_anomaly_detection_param = HttpAnomalyDetectionParam {
            consecutive_5xx: 3,
            base_anomaly_detection_param: BaseAnomalyDetectionParam { ejection_second: 3 },
        };
        let result = base_route.trigger_http_anomaly_detection(
            http_anomaly_detection_param.clone(),
            liveness_status_lock.clone(),
            true,
            LivenessConfig {
                min_liveness_count: 3,
            },
        );
        assert_eq!(result.is_ok(), true);
        let anomaly_detection_status1 = base_route.anomaly_detection_status.read().unwrap();
        assert_eq!(anomaly_detection_status1.consecutive_5xx, 2);
        drop(anomaly_detection_status1);
        let is_alive_option1 = base_route.is_alive.read().unwrap();
        assert_eq!(is_alive_option1.unwrap(), true);
        drop(is_alive_option1);
        let liveness_status1 = liveness_status_lock.read().unwrap();
        assert_eq!(liveness_status1.current_liveness_count, 4);
        drop(liveness_status1);

        let result2 = base_route.trigger_http_anomaly_detection(
            http_anomaly_detection_param,
            liveness_status_lock.clone(),
            true,
            LivenessConfig {
                min_liveness_count: 3,
            },
        );
        // println!("dead_lock :{}", result2.unwrap_err().to_string());
        assert_eq!(result2.is_ok(), true);
        let anomaly_detection_status2 = base_route.anomaly_detection_status.read().unwrap();
        assert_eq!(anomaly_detection_status2.consecutive_5xx, 2);
        drop(anomaly_detection_status2);

        let is_alive_option2 = base_route.is_alive.read().unwrap();
        assert_eq!(is_alive_option2.unwrap(), false);
        drop(is_alive_option2);

        let liveness_status2 = liveness_status_lock.read().unwrap();
        assert_eq!(liveness_status2.current_liveness_count, 3);
        drop(liveness_status2);

        sleep(Duration::from_secs(4)).await;
        let anomaly_detection_status3 = base_route.anomaly_detection_status.read().unwrap();
        assert_eq!(anomaly_detection_status3.consecutive_5xx, 0);
        let is_alive_option3 = base_route.is_alive.read().unwrap();
        assert_eq!(is_alive_option3.unwrap(), true);
        let liveness_status3 = liveness_status_lock.read().unwrap();
        assert_eq!(liveness_status3.current_liveness_count, 4);
    }
}
