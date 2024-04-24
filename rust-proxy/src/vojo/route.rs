use super::app_error::AppError;

use core::fmt::Debug;
use http::HeaderMap;
use http::HeaderValue;
use rand::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use tracing::metadata::LevelFilter;
use uuid::Uuid;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::enum_variant_names)]
#[serde(tag = "type")]
pub enum LoadbalancerStrategy {
    PollRoute(PollRoute),
    HeaderRoute(HeaderRoute),
    RandomRoute(RandomRoute),
    WeightRoute(WeightRoute),
}

impl LoadbalancerStrategy {
    pub async fn get_route(
        &mut self,
        headers: HeaderMap<HeaderValue>,
    ) -> Result<BaseRoute, AppError> {
        match self {
            LoadbalancerStrategy::PollRoute(poll_route) => poll_route.get_route(headers).await,

            LoadbalancerStrategy::HeaderRoute(poll_route) => poll_route.get_route(headers).await,

            LoadbalancerStrategy::RandomRoute(poll_route) => poll_route.get_route(headers).await,

            LoadbalancerStrategy::WeightRoute(poll_route) => poll_route.get_route(headers).await,
        }
    }
    pub fn get_all_route(&mut self) -> Result<Vec<&mut BaseRoute>, AppError> {
        match self {
            LoadbalancerStrategy::PollRoute(poll_route) => poll_route.get_all_route(),
            LoadbalancerStrategy::HeaderRoute(poll_route) => poll_route.get_all_route(),

            LoadbalancerStrategy::RandomRoute(poll_route) => poll_route.get_all_route(),

            LoadbalancerStrategy::WeightRoute(poll_route) => poll_route.get_all_route(),
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
    #[serde(default = "default_base_route_id")]
    pub base_route_id: String,
    #[serde(skip_deserializing)]
    pub is_alive: Option<bool>,
    #[serde(skip_serializing, skip_deserializing)]
    pub anomaly_detection_status: AnomalyDetectionStatus,
}
fn default_base_route_id() -> String {
    let id = Uuid::new_v4();
    id.to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WeightRouteNestedItem {
    pub base_route: BaseRoute,
    pub weight: u64,
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
pub struct HeaderRouteNestedItem {
    pub base_route: BaseRoute,
    pub header_key: String,
    pub header_value_mapping_type: HeaderValueMappingType,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HeaderRoute {
    pub routes: Vec<HeaderRouteNestedItem>,
}

impl HeaderRoute {
    fn get_all_route(&mut self) -> Result<Vec<&mut BaseRoute>, AppError> {
        let vecs = self
            .routes
            .iter_mut()
            .map(|item| &mut item.base_route)
            .collect::<Vec<&mut BaseRoute>>();
        Ok(vecs)
    }

    async fn get_route(&mut self, headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, AppError> {
        let mut alive_cluster: Vec<HeaderRouteNestedItem> = vec![];
        for item in self.routes.clone() {
            let is_alve_result = item.base_route.is_alive;
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
    fn get_all_route(&mut self) -> Result<Vec<&mut BaseRoute>, AppError> {
        let vecs = self
            .routes
            .iter_mut()
            .map(|item| &mut item.base_route)
            .collect::<Vec<&mut BaseRoute>>();
        Ok(vecs)
    }

    async fn get_route(&mut self, _headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, AppError> {
        let mut alive_cluster: Vec<BaseRoute> = vec![];
        for item in self.routes.clone() {
            let is_alve_result = item.base_route.is_alive;
            let is_alive = is_alve_result.unwrap_or(true);
            if is_alive {
                alive_cluster.push(item.base_route.clone());
            }
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
    fn get_all_route(&mut self) -> Result<Vec<&mut BaseRoute>, AppError> {
        let vecs = self
            .routes
            .iter_mut()
            .map(|item| &mut item.base_route)
            .collect::<Vec<&mut BaseRoute>>();
        Ok(vecs)
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
        let level_filter = tracing_subscriber::filter::LevelFilter::current();

        if level_filter == LevelFilter::DEBUG {
            debug!(
                "PollRoute current index:{},cluter len:{},older index:{}",
                current_index as i32, len, older
            );
        }
        Ok(dst.base_route)
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WeightRoute {
    pub routes: Vec<WeightRouteNestedItem>,
    pub index: u64,
    pub offset: u64,
}

impl WeightRoute {
    fn get_all_route(&mut self) -> Result<Vec<&mut BaseRoute>, AppError> {
        let vecs = self
            .routes
            .iter_mut()
            .map(|item| &mut item.base_route)
            .collect::<Vec<&mut BaseRoute>>();
        Ok(vecs)
    }

    async fn get_route(&mut self, _headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, AppError> {
        let cluster_read_lock2 = self.routes.clone();
        loop {
            let currnet_index = self.index;
            let offset = self.offset;
            let current_weight = cluster_read_lock2
                .get(currnet_index as usize)
                .ok_or(AppError(String::from("")))?;
            let is_alive = current_weight.base_route.is_alive.unwrap_or(true);
            if current_weight.weight > offset && is_alive {
                self.offset += 1;
                return Ok(current_weight.base_route.clone());
            }
            if current_weight.weight <= offset {
                self.offset = 0;
                self.index = (self.index + 1) % cluster_read_lock2.len() as u64;
                continue;
            }
            if !is_alive {
                self.offset = 0;
                self.index = (self.index + 1) % cluster_read_lock2.len() as u64;
                continue;
            }
        }

        Err(AppError(String::from("WeightRoute get route error")))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
            let is_alive = base_route.is_alive;
            let anomaly_detection_status = base_route.anomaly_detection_status;
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
                        is_alive: None,
                        base_route_id: String::from(""),
                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    }
                },
            },
            RandomBaseRoute {
                base_route: {
                    BaseRoute {
                        endpoint: String::from("http://localhost:5555"),
                        try_file: None,
                        is_alive: None,
                        base_route_id: String::from(""),

                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    }
                },
            },
            RandomBaseRoute {
                base_route: {
                    BaseRoute {
                        endpoint: String::from("http://localhost:5555"),
                        try_file: None,
                        is_alive: None,
                        base_route_id: String::from(""),

                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
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
                        is_alive: None,
                        base_route_id: String::from(""),

                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    }
                },
            },
            PollBaseRoute {
                base_route: {
                    BaseRoute {
                        endpoint: String::from("http://localhost:5555"),
                        try_file: None,
                        is_alive: None,
                        base_route_id: String::from(""),

                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    }
                },
            },
            PollBaseRoute {
                base_route: {
                    BaseRoute {
                        endpoint: String::from("http://localhost:6666"),
                        try_file: None,
                        is_alive: None,
                        base_route_id: String::from(""),

                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    }
                },
            },
        ]
    }
    fn get_weight_routes() -> Vec<WeightRouteNestedItem> {
        vec![
            WeightRouteNestedItem {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:4444"),
                    try_file: None,
                    is_alive: None,
                    base_route_id: String::from(""),

                    anomaly_detection_status: AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    },
                },
                weight: 100,
            },
            WeightRouteNestedItem {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:5555"),
                    anomaly_detection_status: AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    },
                    base_route_id: String::from(""),

                    try_file: None,
                    is_alive: None,
                },
                weight: 100,
            },
            WeightRouteNestedItem {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:6666"),
                    try_file: None,
                    is_alive: None,
                    base_route_id: String::from(""),

                    anomaly_detection_status: AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    },
                },
                weight: 100,
            },
        ]
    }
    fn get_header_based_routes() -> Vec<HeaderRouteNestedItem> {
        vec![
            HeaderRouteNestedItem {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:4444"),
                    try_file: None,
                    is_alive: None,
                    base_route_id: String::from(""),

                    anomaly_detection_status: AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    },
                },
                header_key: String::from("x-client"),
                header_value_mapping_type: HeaderValueMappingType::Regex(RegexMatch {
                    value: String::from("^100*"),
                }),
            },
            HeaderRouteNestedItem {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:5555"),
                    try_file: None,
                    is_alive: None,
                    base_route_id: String::from(""),

                    anomaly_detection_status: AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    },
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
            HeaderRouteNestedItem {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:7777"),
                    try_file: None,
                    is_alive: None,
                    base_route_id: String::from(""),

                    anomaly_detection_status: AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    },
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
            HeaderRouteNestedItem {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:8888"),
                    try_file: None,
                    is_alive: None,
                    base_route_id: String::from(""),

                    anomaly_detection_status: AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    },
                },
                header_key: String::from("x-client"),
                header_value_mapping_type: HeaderValueMappingType::Text(TextMatch {
                    value: String::from("google chrome"),
                }),
            },
        ]
    }

    #[tokio::test]
    async fn test_poll_route_successfully() {
        let routes = get_poll_routes();
        let mut poll_rate = PollRoute {
            current_index: -1,
            routes: routes.clone(),
        };
        for i in 0..100 {
            let current_route = poll_rate.get_route(HeaderMap::new()).await.unwrap();

            let another_route_vistor = routes[i % routes.len()].base_route.clone();
            assert_eq!(another_route_vistor, current_route);
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
        let mut weight_route = WeightRoute {
            routes: routes.clone(),
            index: 0,
            offset: 0,
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
        let header_route = HeaderRoute { routes };
        let mut header_route = LoadbalancerStrategy::HeaderRoute(header_route);
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
    async fn test_all_route_successfully() {
        {
            let header_route = get_header_based_routes();
            let mut header_wrapper = LoadbalancerStrategy::HeaderRoute(HeaderRoute {
                routes: header_route,
            });
            let routes = header_wrapper.get_all_route();
            assert!(routes.is_ok());
            assert_eq!(routes.unwrap().len(), 4);
        }
        {
            let vecs = get_random_routes();
            let random_route = RandomRoute { routes: vecs };
            let mut header_wrapper = LoadbalancerStrategy::RandomRoute(random_route);
            let routes = header_wrapper.get_all_route();
            assert!(routes.is_ok());
            assert_eq!(routes.unwrap().len(), 3);
        }
        {
            let vecs: Vec<PollBaseRoute> = get_poll_routes();
            let poll_route = PollRoute {
                routes: vecs,
                current_index: 0,
            };
            let mut header_wrapper = LoadbalancerStrategy::PollRoute(poll_route);
            let routes = header_wrapper.get_all_route();
            assert!(routes.is_ok());
            assert_eq!(routes.unwrap().len(), 3);
        }
        {
            let weight_route = get_weight_routes();
            let vecs: Vec<PollBaseRoute> = get_poll_routes();
            let weight_route = WeightRoute {
                routes: weight_route,
                index: 0,
                offset: 0,
            };
            let mut header_wrapper = LoadbalancerStrategy::WeightRoute(weight_route);
            let routes = header_wrapper.get_all_route();
            assert!(routes.is_ok());
            assert_eq!(routes.unwrap().len(), 3);
        }
    }
}
