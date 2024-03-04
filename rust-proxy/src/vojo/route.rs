use super::app_config::LivenessConfig;
use super::app_config::LivenessStatus;
use super::app_config_vistor::BaseRouteVistor;
use super::app_error::AppError;
use crate::vojo::anomaly_detection::HttpAnomalyDetectionParam;
use crate::vojo::app_config_vistor::{
    HeaderBasedRouteVistor, HeaderRouteVistor, PollBaseRouteVistor, PollRouteVistor,
    RandomBaseRouteVistor, RandomRouteVistor, WeightBasedRouteVistor, WeightRouteVistor,
};
use core::fmt::Debug;
use http::HeaderMap;
use http::HeaderValue;
use log::Level;
use rand::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
#[derive(Debug, Clone)]
pub enum LoadbalancerStrategy {
    PollRoute(PollRoute),
    HeaderBased(HeaderBasedRoute),
    Random(RandomRoute),
    WeightBased(WeightBasedRoute),
}

impl LoadbalancerStrategy {
    pub async fn get_route(
        &mut self,
        headers: HeaderMap<HeaderValue>,
    ) -> Result<BaseRoute, AppError> {
        match self {
            LoadbalancerStrategy::PollRoute(poll_route) => poll_route.get_route(headers).await,

            LoadbalancerStrategy::HeaderBased(poll_route) => poll_route.get_route(headers).await,

            LoadbalancerStrategy::Random(poll_route) => poll_route.get_route(headers).await,

            LoadbalancerStrategy::WeightBased(poll_route) => poll_route.get_route(headers).await,
        }
    }
    pub async fn get_all_route(&mut self) -> Result<Vec<BaseRoute>, AppError> {
        match self {
            LoadbalancerStrategy::PollRoute(poll_route) => poll_route.get_all_route().await,
            LoadbalancerStrategy::HeaderBased(poll_route) => poll_route.get_all_route().await,

            LoadbalancerStrategy::Random(poll_route) => poll_route.get_all_route().await,

            LoadbalancerStrategy::WeightBased(poll_route) => poll_route.get_all_route().await,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AnomalyDetectionStatus {
    pub consecutive_5xx: i32,
}
#[derive(Debug, Clone, Deserialize, Default)]
pub struct BaseRoute {
    pub endpoint: String,
    pub try_file: Option<String>,
    #[serde(skip_deserializing)]
    pub is_alive: Arc<RwLock<Option<bool>>>,
    #[serde(skip_serializing, skip_deserializing)]
    pub anomaly_detection_status: Arc<RwLock<AnomalyDetectionStatus>>,
}
impl BaseRoute {
    pub fn from(base_route_vistor: BaseRouteVistor) -> Self {
        BaseRoute {
            endpoint: base_route_vistor.endpoint,
            try_file: base_route_vistor.try_file,
            is_alive: Arc::new(RwLock::new(base_route_vistor.is_alive)),
            anomaly_detection_status: Arc::new(RwLock::new(
                base_route_vistor.anomaly_detection_status,
            )),
        }
    }
}

impl BaseRoute {
    async fn update_ok(&self, liveness_status_lock: Arc<RwLock<LivenessStatus>>) -> bool {
        let mut is_alive_lock = self.is_alive.write().await;
        if is_alive_lock.is_none() {
            *is_alive_lock = Some(true);
            info!(
                "Update the liveness of route-{} to ok succesfully!",
                self.endpoint.clone(),
            );
            return true;
        } else if !is_alive_lock.unwrap() {
            *is_alive_lock = Some(true);
            let mut liveness_status = liveness_status_lock.write().await;
            liveness_status.current_liveness_count += 1;
            info!(
                "Update the liveness of route-{} to ok succesfully,and the current liveness count is {}.",
                self.endpoint.clone(),liveness_status.current_liveness_count);
            return true;
        }
        false
    }
    async fn update_fail(&self, liveness_status_lock: Arc<RwLock<LivenessStatus>>) -> bool {
        let mut is_alive_lock = self.is_alive.write().await;
        if is_alive_lock.is_none() || is_alive_lock.unwrap() {
            let mut liveness_status = liveness_status_lock.write().await;
            liveness_status.current_liveness_count -= 1;
            *is_alive_lock = Some(false);
            info!(
                "Update the liveness of route-{} to fail succesfully,and the current liveness count is {}.",
                self.endpoint.clone(),liveness_status.current_liveness_count);
            return true;
        }
        false
    }
    pub async fn update_health_check_status_with_ok(
        &self,
        liveness_status_lock: Arc<RwLock<LivenessStatus>>,
    ) -> bool {
        let is_alive_lock = self.is_alive.read().await;
        let is_alive = is_alive_lock.unwrap_or(false);
        if !is_alive {
            drop(is_alive_lock);
            self.update_ok(liveness_status_lock).await
        } else {
            info!(
            "Update the liveness of route-{} to ok unsuccesfully,as the current status of the endpoint is alive!",
            self.endpoint.clone(),
        );
            false
        }
    }
    pub async fn update_health_check_status_with_fail(
        &self,
        liveness_status_lock: Arc<RwLock<LivenessStatus>>,
        liveness_config: LivenessConfig,
    ) -> bool {
        let liveness_status = liveness_status_lock.read().await;
        if liveness_status.current_liveness_count <= liveness_config.min_liveness_count {
            error!(
                "Update the liveness of route-{} to fail unsuccesfully,as the current liveness count:{} is less than the liveness count:{} in the config!",
                self.endpoint.clone(),
                liveness_status.current_liveness_count,
                liveness_config.min_liveness_count
            );
            return false;
        }
        let is_alive_lock = self.is_alive.read().await;
        let is_alive = is_alive_lock.unwrap_or(true);
        if is_alive {
            drop(liveness_status);
            drop(is_alive_lock);
            self.update_fail(liveness_status_lock.clone()).await;
            info!(
                "Update the liveness of route-{} to fail succesfully!",
                self.endpoint.clone()
            );
            return true;
        } else {
            info!(
            "Update the liveness of route-{} to fail unsuccesfully,as the current status of the endpoint is not alive!",
            self.endpoint.clone(),
        );
        }
        false
    }
    pub async fn trigger_http_anomaly_detection(
        &self,
        http_anomaly_detection_param: HttpAnomalyDetectionParam,
        liveness_status_lock: Arc<RwLock<LivenessStatus>>,
        is_5xx: bool,
        liveness_config: LivenessConfig,
    ) -> Result<(), AppError> {
        let consecutive_5xx_config = http_anomaly_detection_param.consecutive_5xx;
        let mut anomaly_detection_status = self
            .anomaly_detection_status
            .try_write()
            .map_err(|e| AppError(e.to_string()))?;
        if !is_5xx && anomaly_detection_status.consecutive_5xx > 0 {
            anomaly_detection_status.consecutive_5xx = 0;
            return Ok(());
        }

        if anomaly_detection_status.consecutive_5xx < consecutive_5xx_config - 1 {
            anomaly_detection_status.consecutive_5xx += 1;
        } else {
            drop(anomaly_detection_status);
            let update_success = self
                .update_health_check_status_with_fail(liveness_status_lock.clone(), liveness_config)
                .await;
            if update_success {
                let alive_lock = self.is_alive.clone();
                let ejection_second = http_anomaly_detection_param
                    .base_anomaly_detection_param
                    .ejection_second;
                let anomaly_detection_status_lock = self.anomaly_detection_status.clone();
                tokio::spawn(async move {
                    BaseRoute::wait_for_alive(
                        alive_lock,
                        ejection_second,
                        liveness_status_lock,
                        anomaly_detection_status_lock,
                    )
                    .await;
                    info!("Wait for alive successfully!");
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
    ) {
        sleep(Duration::from_secs(wait_second)).await;
        let mut is_alive_option = is_alive_lock.write().await;
        let mut liveness_status = liveness_status_lock.write().await;
        let mut anomaly_detection_status = anomaly_detection_status_lock.write().await;
        *is_alive_option = Some(true);
        liveness_status.current_liveness_count += 1;
        anomaly_detection_status.consecutive_5xx = 0;
    }
}

#[derive(Debug, Clone, Default)]
pub struct WeightRoute {
    pub base_route: BaseRoute,
    pub weight: i32,
    pub index: Arc<AtomicIsize>,
}
impl WeightRoute {
    pub fn new_list(weight_route_vistors: Vec<WeightRouteVistor>) -> Vec<WeightRoute> {
        weight_route_vistors
            .iter()
            .map(|item| WeightRoute {
                base_route: BaseRoute::from(item.base_route.clone()),
                weight: item.weight,
                index: Arc::new(AtomicIsize::new(item.index as isize)),
            })
            .collect::<Vec<WeightRoute>>()
    }
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
    Regex(RegexMatch),
    Text(TextMatch),
    Split(SplitSegment),
}
#[derive(Debug, Clone)]
pub struct HeaderRoute {
    pub base_route: BaseRoute,
    pub header_key: String,
    pub header_value_mapping_type: HeaderValueMappingType,
}
impl HeaderRoute {
    pub fn new_list(header_route_vistors: Vec<HeaderRouteVistor>) -> Vec<HeaderRoute> {
        header_route_vistors
            .iter()
            .map(|item| HeaderRoute {
                base_route: BaseRoute::from(item.base_route.clone()),
                header_key: item.header_key.clone(),
                header_value_mapping_type: item.header_value_mapping_type.clone(),
            })
            .collect::<Vec<HeaderRoute>>()
    }
}

#[derive(Debug, Clone, Default)]
pub struct HeaderBasedRoute {
    pub routes: Vec<HeaderRoute>,
}
impl HeaderBasedRoute {
    pub fn from(header_based_route_vistor: HeaderBasedRouteVistor) -> Self {
        HeaderBasedRoute {
            routes: HeaderRoute::new_list(header_based_route_vistor.routes),
        }
    }
}
// #[typetag::serde]
// #[async_trait]
impl HeaderBasedRoute {
    async fn get_all_route(&mut self) -> Result<Vec<BaseRoute>, AppError> {
        Ok(self
            .routes
            .iter()
            .map(|item| item.base_route.clone())
            .collect::<Vec<BaseRoute>>())
    }

    async fn get_route(&mut self, headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, AppError> {
        let mut alive_cluster: Vec<HeaderRoute> = vec![];
        for item in self.routes.clone() {
            let is_alve_result = item.base_route.is_alive.read().await;
            // let is_alive_option = is_alve_result.unwrap();
            let is_alive = is_alve_result.unwrap_or(true);
            if is_alive {
                alive_cluster.push(item.clone());
            }
        }
        for item in alive_cluster.iter() {
            let headers_contais_key = headers.contains_key(item.header_key.clone());
            if !headers_contais_key {
                continue;
            }
            let header_value = headers.get(item.header_key.clone()).unwrap();
            let header_value_str = header_value.to_str().unwrap();
            match item.clone().header_value_mapping_type {
                HeaderValueMappingType::Regex(regex_str) => {
                    let re = Regex::new(&regex_str.value).unwrap();
                    let capture_option = re.captures(header_value_str);
                    if capture_option.is_none() {
                        continue;
                    } else {
                        return Ok(item.clone().base_route);
                    }
                }
                HeaderValueMappingType::Text(text_str) => {
                    if text_str.value == header_value_str {
                        return Ok(item.clone().base_route);
                    } else {
                        continue;
                    }
                }
                HeaderValueMappingType::Split(split_segment) => {
                    let split_set: HashSet<_> =
                        header_value_str.split(&split_segment.split_by).collect();
                    if split_set.is_empty() {
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

        let first = alive_cluster.first().unwrap().base_route.clone();
        Ok(first)
    }
}
#[derive(Debug, Clone, Default)]
pub struct RandomBaseRoute {
    pub base_route: BaseRoute,
}
#[derive(Debug, Clone, Default)]
pub struct RandomRoute {
    pub routes: Vec<RandomBaseRoute>,
}
impl RandomBaseRoute {
    pub fn new_list(random_base_route_vistors: Vec<RandomBaseRouteVistor>) -> Vec<RandomBaseRoute> {
        random_base_route_vistors
            .iter()
            .map(|item| RandomBaseRoute {
                base_route: BaseRoute::from(item.base_route.clone()),
            })
            .collect::<Vec<RandomBaseRoute>>()
    }
}
impl RandomRoute {
    pub fn from(random_route_vistor: RandomRouteVistor) -> Self {
        RandomRoute {
            routes: RandomBaseRoute::new_list(random_route_vistor.routes),
        }
    }
}

impl RandomRoute {
    async fn get_all_route(&mut self) -> Result<Vec<BaseRoute>, AppError> {
        Ok(self
            .routes
            .iter()
            .map(|item| item.base_route.clone())
            .collect::<Vec<BaseRoute>>())
    }

    async fn get_route(&mut self, _headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, AppError> {
        let mut alive_cluster: Vec<BaseRoute> = vec![];
        for item in self.routes.clone() {
            let is_alve_result = item.base_route.is_alive.read().await;
            let is_alive = is_alve_result.unwrap_or(true);
            if is_alive {
                alive_cluster.push(item.base_route.clone());
            }
            drop(is_alve_result);
        }
        let mut rng = thread_rng();
        let index = rng.gen_range(0..alive_cluster.len());
        let dst = alive_cluster[index].clone();
        Ok(dst)
    }
}
#[derive(Debug, Clone, Default)]
pub struct PollBaseRoute {
    pub base_route: BaseRoute,
}
impl PollBaseRoute {
    pub fn new_list(poll_base_route_vistors: Vec<PollBaseRouteVistor>) -> Vec<PollBaseRoute> {
        poll_base_route_vistors
            .iter()
            .map(|item| PollBaseRoute {
                base_route: BaseRoute::from(item.base_route.clone()),
            })
            .collect::<Vec<PollBaseRoute>>()
    }
}
#[derive(Debug, Clone, Default)]
pub struct PollRoute {
    pub current_index: Arc<AtomicUsize>,
    pub routes: Vec<PollBaseRoute>,
}
impl PollRoute {
    pub fn from(poll_route_vistor: PollRouteVistor) -> Self {
        PollRoute {
            current_index: Arc::new(AtomicUsize::new(poll_route_vistor.current_index as usize)),
            routes: PollBaseRoute::new_list(poll_route_vistor.routes),
        }
    }
}

impl PollRoute {
    async fn get_all_route(&mut self) -> Result<Vec<BaseRoute>, AppError> {
        Ok(self
            .routes
            .iter_mut()
            .map(|item| item.base_route.clone())
            .collect::<Vec<BaseRoute>>())
    }

    async fn get_route(&mut self, _headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, AppError> {
        let mut alive_cluster: Vec<PollBaseRoute> = vec![];
        for item in self.routes.clone() {
            let is_alve_result = item.base_route.is_alive.read().await;
            let is_alive = is_alve_result.unwrap_or(true);
            if is_alive {
                alive_cluster.push(item.clone());
            }
        }
        if alive_cluster.is_empty() {
            return Err(AppError(String::from(
                "Can not find alive host in the clusters",
            )));
        }
        let older = self.current_index.fetch_add(1, Ordering::SeqCst);
        let len = alive_cluster.len();
        let current_index = older % len;
        let dst = alive_cluster[current_index].clone();
        if log_enabled!(Level::Debug) {
            debug!("PollRoute current index:{}", current_index as i32);
        }
        Ok(dst.base_route)
    }
}
#[derive(Debug, Clone, Default)]
pub struct WeightBasedRoute {
    pub routes: Arc<RwLock<Vec<WeightRoute>>>,
}
impl WeightBasedRoute {
    pub fn from(weight_based_route_vistor: WeightBasedRouteVistor) -> Self {
        WeightBasedRoute {
            routes: Arc::new(RwLock::new(WeightRoute::new_list(
                weight_based_route_vistor.routes,
            ))),
        }
    }
}

impl WeightBasedRoute {
    async fn get_all_route(&mut self) -> Result<Vec<BaseRoute>, AppError> {
        let read_lock = self.routes.read().await;
        let array = read_lock
            .iter()
            .map(|item| item.base_route.clone())
            .collect::<Vec<BaseRoute>>();
        Ok(array)
    }

    async fn get_route(&mut self, _headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, AppError> {
        let cluster_read_lock = self.routes.read().await;
        let mut all_cluster_dead = true;
        for (pos, e) in cluster_read_lock.iter().enumerate() {
            let is_alive_option_lock = e.base_route.is_alive.read().await;
            let is_alive = is_alive_option_lock.unwrap_or(true);
            if is_alive {
                all_cluster_dead = false;
                let old_value = e.index.fetch_sub(1, Ordering::SeqCst);
                if old_value > 0 {
                    if log_enabled!(Level::Debug) {
                        debug!("WeightRoute current index:{}", pos as i32);
                    }
                    return Ok(e.base_route.clone());
                }
            }
        }

        drop(cluster_read_lock);
        if all_cluster_dead {
            return Err(AppError(String::from("There are no alive host!")));
        }
        let mut new_lock = self.routes.write().await;
        let index_is_alive = new_lock.iter().any(|f| {
            let tt = f.index.load(Ordering::SeqCst);
            tt.is_positive()
        });
        if !index_is_alive {
            (*new_lock).iter_mut().for_each(|weight_route| {
                weight_route.index = Arc::new(AtomicIsize::from(weight_route.weight as isize))
            });
        }
        drop(new_lock);
        let cluster_read_lock2 = self.routes.read().await;

        for (pos, e) in cluster_read_lock2.iter().enumerate() {
            let is_alive_option_lock = e.base_route.is_alive.read().await;
            let is_alive = is_alive_option_lock.unwrap_or(true);
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
        Err(AppError(String::from("WeightRoute get route error")))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vojo::anomaly_detection::BaseAnomalyDetectionParam;
    use std::vec;
    #[derive(PartialEq, Eq, Debug)]
    pub struct BaseRouteWithoutLock {
        pub endpoint: String,
        pub try_file: Option<String>,
        pub is_alive: Option<bool>,
        pub anomaly_detection_status: AnomalyDetectionStatus,
    }
    impl BaseRouteWithoutLock {
        async fn new(base_route: BaseRoute) -> Self {
            let is_alive = *base_route.is_alive.read().await;
            let anomaly_detection_status = base_route.anomaly_detection_status.read().await.clone();
            BaseRouteWithoutLock {
                endpoint: base_route.endpoint,
                try_file: base_route.try_file,
                is_alive,
                anomaly_detection_status,
            }
        }
    }

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
                header_value_mapping_type: HeaderValueMappingType::Regex(RegexMatch {
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
                header_value_mapping_type: HeaderValueMappingType::Split(SplitSegment {
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
                header_value_mapping_type: HeaderValueMappingType::Split(SplitSegment {
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
                header_value_mapping_type: HeaderValueMappingType::Text(TextMatch {
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
    #[tokio::test]
    async fn test_poll_route_successfully() {
        let routes = get_poll_routes();
        let mut poll_rate = PollRoute {
            current_index: Default::default(),
            routes: routes.clone(),
        };
        for i in 0..100 {
            let current_route = poll_rate.get_route(HeaderMap::new()).await.unwrap();
            let current_route_vistor = BaseRouteVistor::from(current_route).await;
            let another_route_vistor =
                BaseRouteVistor::from(routes[i % routes.len()].base_route.clone()).await;
            assert_eq!(another_route_vistor, current_route_vistor);
        }
    }
    #[tokio::test]
    async fn test_random_route_successfully() {
        let routes = get_random_routes();
        let mut random_rate = RandomRoute { routes };
        for _ in 0..100 {
            random_rate.get_route(HeaderMap::new()).await.unwrap();
        }
    }
    #[tokio::test]
    async fn test_weight_route_successfully() {
        let routes = get_weight_routes();
        let mut weight_route = WeightBasedRoute {
            routes: Arc::new(RwLock::new(routes.clone())),
        };

        for _ in 0..100 {
            let current_route = weight_route.get_route(HeaderMap::new()).await.unwrap();
            assert_eq!(
                BaseRouteWithoutLock::new(current_route).await,
                BaseRouteWithoutLock::new(routes[0].base_route.clone()).await
            );
        }
        for _ in 0..100 {
            let current_route = weight_route.get_route(HeaderMap::new()).await.unwrap();
            assert_eq!(
                BaseRouteWithoutLock::new(current_route.clone()).await,
                BaseRouteWithoutLock::new(routes[1].base_route.clone()).await
            );
        }
        for _ in 0..100 {
            let current_route = weight_route.get_route(HeaderMap::new()).await.unwrap();

            assert_eq!(
                BaseRouteWithoutLock::new(current_route.clone()).await,
                BaseRouteWithoutLock::new(routes[2].base_route.clone()).await
            );
        }
        for _ in 0..100 {
            let current_route = weight_route.get_route(HeaderMap::new()).await.unwrap();

            assert_eq!(
                BaseRouteWithoutLock::new(current_route).await,
                BaseRouteWithoutLock::new(routes[0].base_route.clone()).await
            );
        }
    }

    #[tokio::test]
    async fn test_header_based_route_successfully() {
        let routes = get_header_based_routes();
        let header_route = HeaderBasedRoute { routes };
        let mut header_route = LoadbalancerStrategy::HeaderBased(header_route);
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("x-client", "100zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap());
        let result1 = header_route.get_route(headermap1.clone()).await;
        assert!(result1.is_ok());
        assert_eq!(result1.unwrap().endpoint, "http://localhost:4444");

        let mut headermap2 = HeaderMap::new();
        headermap2.insert("x-client", "a=1;b=2;c:3;d=4;f5=6667".parse().unwrap());
        let result2 = header_route.get_route(headermap2.clone()).await;
        assert!(result2.is_ok());
        assert_eq!(result2.unwrap().endpoint, "http://localhost:5555");

        let mut headermap3 = HeaderMap::new();
        headermap3.insert("x-client", "a:12,b:9,c=7,d=4;f5=6667".parse().unwrap());
        let result3 = header_route.get_route(headermap3.clone()).await;
        assert!(result3.is_ok());
        assert_eq!(result3.unwrap().endpoint, "http://localhost:7777");

        let mut headermap4 = HeaderMap::new();
        headermap4.insert("x-client", "google chrome".parse().unwrap());
        let result4 = header_route.get_route(headermap4.clone()).await;
        assert!(result4.is_ok());
        assert_eq!(result4.unwrap().endpoint, "http://localhost:8888");
    }
    #[tokio::test]
    async fn test_update_health_check_status_with_ok_success1() {
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

        base_route
            .update_health_check_status_with_ok(liveness_status_lock.clone())
            .await;
        let is_alive_option = base_route.is_alive.read().await;
        assert!(is_alive_option.is_some(),);
        assert!(is_alive_option.unwrap(),);
        let liveness_status = liveness_status_lock.read().await;
        assert_eq!(liveness_status.current_liveness_count, 3);
    }
    #[tokio::test]
    async fn test_update_health_check_status_with_ok_success2() {
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

        base_route
            .update_health_check_status_with_ok(liveness_status_lock.clone())
            .await;
        let is_alive_option = base_route.is_alive.read().await;
        assert!(is_alive_option.is_some(),);
        assert!(is_alive_option.unwrap(),);
        let liveness_status = liveness_status_lock.read().await;
        assert_eq!(liveness_status.current_liveness_count, 3);
    }
    #[tokio::test]
    async fn test_update_health_check_status_with_ok_success3() {
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

        base_route
            .update_health_check_status_with_ok(liveness_status_lock.clone())
            .await;
        let is_alive_option = base_route.is_alive.read().await;
        assert!(is_alive_option.is_some(),);
        assert!(is_alive_option.unwrap(),);
        let liveness_status = liveness_status_lock.read().await;
        assert_eq!(liveness_status.current_liveness_count, 4);
    }

    #[tokio::test]
    async fn test_update_health_check_status_with_fail_success1() {
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

        let result = base_route
            .update_health_check_status_with_fail(
                liveness_status_lock,
                LivenessConfig {
                    min_liveness_count: 3,
                },
            )
            .await;
        assert!(!result);
        let is_alive_option = base_route.is_alive.read().await;
        assert!(is_alive_option.is_none());
    }
    #[tokio::test]
    async fn test_update_health_check_status_with_fail_success2() {
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

        let result = base_route
            .update_health_check_status_with_fail(
                liveness_status_lock.clone(),
                LivenessConfig {
                    min_liveness_count: 3,
                },
            )
            .await;
        //update fails
        assert!(!result);
        let is_alive_option = base_route.is_alive.read().await;
        assert!(is_alive_option.is_some());
        assert!(is_alive_option.unwrap());
        let liveness_status = liveness_status_lock.read().await;
        assert_eq!(liveness_status.current_liveness_count, 3);
    }
    #[tokio::test]
    async fn test_update_health_check_status_with_fail_success3() {
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

        let result = base_route
            .update_health_check_status_with_fail(
                liveness_status_lock.clone(),
                LivenessConfig {
                    min_liveness_count: 3,
                },
            )
            .await;
        assert!(!result);
        let is_alive_option = base_route.is_alive.read().await;
        assert!(is_alive_option.is_some(),);
        assert!(!is_alive_option.unwrap(),);
        let liveness_status = liveness_status_lock.read().await;
        assert_eq!(liveness_status.current_liveness_count, 3);
    }

    #[tokio::test]
    async fn test_trigger_http_anomaly_detection_success1() {
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
        let result = base_route
            .trigger_http_anomaly_detection(
                http_anomaly_detection_param,
                liveness_status_lock,
                true,
                LivenessConfig {
                    min_liveness_count: 3,
                },
            )
            .await;
        assert!(result.is_ok());
        let anomaly_detection_status = base_route.anomaly_detection_status.read().await;
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
        let result = base_route
            .trigger_http_anomaly_detection(
                http_anomaly_detection_param.clone(),
                liveness_status_lock.clone(),
                true,
                LivenessConfig {
                    min_liveness_count: 3,
                },
            )
            .await;
        assert!(result.is_ok(),);
        {
            let anomaly_detection_status1 = base_route.anomaly_detection_status.read().await;
            assert_eq!(anomaly_detection_status1.consecutive_5xx, 2);
            drop(anomaly_detection_status1);
            let is_alive_option1 = base_route.is_alive.read().await;
            assert!(is_alive_option1.unwrap(),);
            drop(is_alive_option1);
            let liveness_status1 = liveness_status_lock.read().await;
            assert_eq!(liveness_status1.current_liveness_count, 4);
            drop(liveness_status1);
        }
        {
            let result2 = base_route
                .trigger_http_anomaly_detection(
                    http_anomaly_detection_param,
                    liveness_status_lock.clone(),
                    true,
                    LivenessConfig {
                        min_liveness_count: 3,
                    },
                )
                .await;
            assert!(result2.is_ok(),);
            let anomaly_detection_status2 = base_route.anomaly_detection_status.read().await;
            assert_eq!(anomaly_detection_status2.consecutive_5xx, 2);
            drop(anomaly_detection_status2);

            let is_alive_option2 = base_route.is_alive.read().await;
            assert!(!is_alive_option2.unwrap(),);
            drop(is_alive_option2);

            let liveness_status2 = liveness_status_lock.read().await;
            assert_eq!(liveness_status2.current_liveness_count, 3);
            drop(liveness_status2);
        }
        sleep(Duration::from_secs(4)).await;
        {
            let anomaly_detection_status3 = base_route.anomaly_detection_status.read().await;
            assert_eq!(anomaly_detection_status3.consecutive_5xx, 0);
            let is_alive_option3 = base_route.is_alive.read().await;
            assert!(is_alive_option3.unwrap());
            let liveness_status3 = liveness_status_lock.read().await;
            assert_eq!(liveness_status3.current_liveness_count, 4);
        }
    }
}
