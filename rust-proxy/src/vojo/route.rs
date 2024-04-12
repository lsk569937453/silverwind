use super::app_config::LivenessConfig;
use super::app_config::LivenessStatus;
use super::app_error::AppError;
use crate::vojo::anomaly_detection::HttpAnomalyDetectionParam;

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::enum_variant_names)]
#[serde(tag = "type")]
pub enum LoadbalancerStrategy {
    PollRoute(PollRoute),
    HeaderBasedRoute(HeaderBasedRoute),
    RandomRoute(RandomRoute),
    WeightBasedRoute(WeightBasedRoute),
}

impl LoadbalancerStrategy {
    pub async fn get_route(
        &mut self,
        headers: HeaderMap<HeaderValue>,
    ) -> Result<BaseRoute, AppError> {
        match self {
            LoadbalancerStrategy::PollRoute(poll_route) => poll_route.get_route(headers).await,

            LoadbalancerStrategy::HeaderBasedRoute(poll_route) => {
                poll_route.get_route(headers).await
            }

            LoadbalancerStrategy::RandomRoute(poll_route) => poll_route.get_route(headers).await,

            LoadbalancerStrategy::WeightBasedRoute(poll_route) => {
                poll_route.get_route(headers).await
            }
        }
    }
    pub async fn get_all_route(&mut self) -> Result<Vec<BaseRoute>, AppError> {
        match self {
            LoadbalancerStrategy::PollRoute(poll_route) => poll_route.get_all_route().await,
            LoadbalancerStrategy::HeaderBasedRoute(poll_route) => poll_route.get_all_route().await,

            LoadbalancerStrategy::RandomRoute(poll_route) => poll_route.get_all_route().await,

            LoadbalancerStrategy::WeightBasedRoute(poll_route) => poll_route.get_all_route().await,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AnomalyDetectionStatus {
    pub consecutive_5xx: i32,
}
#[derive(Debug, Clone, Deserialize, Default, PartialEq, Serialize)]
pub struct BaseRoute {
    pub endpoint: String,
    pub try_file: Option<String>,
    #[serde(skip_deserializing)]
    pub is_alive: Option<bool>,
    #[serde(skip_serializing, skip_deserializing)]
    pub anomaly_detection_status: AnomalyDetectionStatus,
}

impl BaseRoute {
    async fn update_ok(&self, liveness_status_lock: Arc<RwLock<LivenessStatus>>) -> bool {
        // let mut is_alive_lock = self.is_alive.write().await;
        // if is_alive_lock.is_none() {
        //     *is_alive_lock = Some(true);
        //     info!(
        //         "Update the liveness of route-{} to ok succesfully!",
        //         self.endpoint.clone(),
        //     );
        //     return true;
        // } else if !is_alive_lock.unwrap() {
        //     *is_alive_lock = Some(true);
        //     let mut liveness_status = liveness_status_lock.write().await;
        //     liveness_status.current_liveness_count += 1;
        //     info!(
        //         "Update the liveness of route-{} to ok succesfully,and the current liveness count is {}.",
        //         self.endpoint.clone(),liveness_status.current_liveness_count);
        //     return true;
        // }
        false
    }
    async fn update_fail(&self, liveness_status_lock: Arc<RwLock<LivenessStatus>>) -> bool {
        // let mut is_alive_lock = self.is_alive.write().await;
        // if is_alive_lock.is_none() || is_alive_lock.unwrap() {
        //     let mut liveness_status = liveness_status_lock.write().await;
        //     liveness_status.current_liveness_count -= 1;
        //     *is_alive_lock = Some(false);
        //     info!(
        //         "Update the liveness of route-{} to fail succesfully,and the current liveness count is {}.",
        //         self.endpoint.clone(),liveness_status.current_liveness_count);
        //     return true;
        // }
        false
    }
    pub async fn update_health_check_status_with_ok(
        &self,
        liveness_status_lock: Arc<RwLock<LivenessStatus>>,
    ) -> bool {
        // let is_alive_lock = self.is_alive.read().await;
        // let is_alive = is_alive_lock.unwrap_or(false);
        // if !is_alive {
        //     drop(is_alive_lock);
        //     self.update_ok(liveness_status_lock).await
        // } else {
        //     info!(
        //     "Update the liveness of route-{} to ok unsuccesfully,as the current status of the endpoint is alive!",
        //     self.endpoint.clone(),
        // );
        false
    }
    pub async fn update_health_check_status_with_fail(
        &self,
        liveness_status_lock: Arc<RwLock<LivenessStatus>>,
        liveness_config: LivenessConfig,
    ) -> bool {
        // let liveness_status = liveness_status_lock.read().await;
        // if liveness_status.current_liveness_count <= liveness_config.min_liveness_count {
        //     error!(
        //         "Update the liveness of route-{} to fail unsuccesfully,as the current liveness count:{} is less than the liveness count:{} in the config!",
        //         self.endpoint.clone(),
        //         liveness_status.current_liveness_count,
        //         liveness_config.min_liveness_count
        //     );
        //     return false;
        // }
        // let is_alive_lock = self.is_alive.read().await;
        // let is_alive = is_alive_lock.unwrap_or(true);
        // if is_alive {
        //     drop(liveness_status);
        //     drop(is_alive_lock);
        //     self.update_fail(liveness_status_lock.clone()).await;
        //     info!(
        //         "Update the liveness of route-{} to fail succesfully!",
        //         self.endpoint.clone()
        //     );
        //     return true;
        // } else {
        //     info!(
        //     "Update the liveness of route-{} to fail unsuccesfully,as the current status of the endpoint is not alive!",
        //     self.endpoint.clone(),
        // );
        // }
        false
    }
    pub async fn trigger_http_anomaly_detection(
        &self,
        http_anomaly_detection_param: HttpAnomalyDetectionParam,
        liveness_status_lock: Arc<RwLock<LivenessStatus>>,
        is_5xx: bool,
        liveness_config: LivenessConfig,
    ) -> Result<(), AppError> {
        // let consecutive_5xx_config = http_anomaly_detection_param.consecutive_5xx;
        // let mut anomaly_detection_status = self
        //     .anomaly_detection_status
        //     .try_write()
        //     .map_err(|e| AppError(e.to_string()))?;
        // if !is_5xx && anomaly_detection_status.consecutive_5xx > 0 {
        //     anomaly_detection_status.consecutive_5xx = 0;
        //     return Ok(());
        // }

        // if anomaly_detection_status.consecutive_5xx < consecutive_5xx_config - 1 {
        //     anomaly_detection_status.consecutive_5xx += 1;
        // } else {
        //     drop(anomaly_detection_status);
        //     let update_success = self
        //         .update_health_check_status_with_fail(liveness_status_lock.clone(), liveness_config)
        //         .await;
        //     if update_success {
        //         let alive_lock = self.is_alive.clone();
        //         let ejection_second = http_anomaly_detection_param
        //             .base_anomaly_detection_param
        //             .ejection_second;
        //         let anomaly_detection_status_lock = self.anomaly_detection_status.clone();
        //         tokio::spawn(async move {
        //             BaseRoute::wait_for_alive(
        //                 alive_lock,
        //                 ejection_second,
        //                 liveness_status_lock,
        //                 anomaly_detection_status_lock,
        //             )
        //             .await;
        //             info!("Wait for alive successfully!");
        //         });
        //     }
        // }
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WeightRoute {
    pub base_route: BaseRoute,
    pub weight: i32,
    pub index: i64,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeaderRoute {
    pub base_route: BaseRoute,
    pub header_key: String,
    pub header_value_mapping_type: HeaderValueMappingType,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HeaderBasedRoute {
    pub routes: Vec<HeaderRoute>,
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
            let is_alve_result = item.base_route.is_alive;
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RandomBaseRoute {
    pub base_route: BaseRoute,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RandomRoute {
    pub routes: Vec<RandomBaseRoute>,
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
            let is_alve_result = item.base_route.is_alive;
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PollBaseRoute {
    pub base_route: BaseRoute,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PollRoute {
    #[serde(skip)]
    pub current_index: i64,
    pub routes: Vec<PollBaseRoute>,
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
            let is_alve_result = item.base_route.is_alive;
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
        let older = self.current_index;
        let len = alive_cluster.len();
        let current_index = (older + 1) % len as i64;
        self.current_index = current_index;
        let dst = alive_cluster[current_index as usize].clone();
        if log_enabled!(Level::Debug) {
            debug!(
                "PollRoute current index:{},cluter len:{},older index:{}",
                current_index as i32, len, older
            );
        }
        Ok(dst.base_route)
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WeightBasedRoute {
    pub routes: Vec<WeightRoute>,
}

impl WeightBasedRoute {
    async fn get_all_route(&mut self) -> Result<Vec<BaseRoute>, AppError> {
        let read_lock = self.routes.clone();
        let array = read_lock
            .iter()
            .map(|item| item.base_route.clone())
            .collect::<Vec<BaseRoute>>();
        Ok(array)
    }

    async fn get_route(&mut self, _headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, AppError> {
        let cluster_read_lock = self.routes.clone();
        let mut all_cluster_dead = true;
        for (pos, e) in cluster_read_lock.iter().enumerate() {
            let is_alive_option_lock = e.base_route.is_alive;
            let is_alive = is_alive_option_lock.unwrap_or(true);
            if is_alive {
                all_cluster_dead = false;
                let old_value = e.index;
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
        let mut new_lock = self.routes.clone();
        let index_is_alive = new_lock.iter().any(|f| {
            let tt = f.index;
            tt.is_positive()
        });
        if !index_is_alive {
            (*new_lock)
                .iter_mut()
                .for_each(|weight_route| weight_route.index = weight_route.weight as i64);
        }
        drop(new_lock);
        let cluster_read_lock2 = self.routes.clone();

        for (pos, e) in cluster_read_lock2.iter().enumerate() {
            let is_alive_option_lock = e.base_route.is_alive;
            let is_alive = is_alive_option_lock.unwrap_or(true);
            if is_alive {
                let old_value = e.index;
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
