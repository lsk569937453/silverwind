use crate::vojo::allow_deny_ip::AllowDenyObject;
use crate::vojo::anomaly_detection::AnomalyDetectionType;
use crate::vojo::app_config::ApiService;
use crate::vojo::app_config::AppConfig;
use crate::vojo::app_config::LivenessConfig;
use crate::vojo::app_config::LivenessStatus;
use crate::vojo::app_config::Matcher;
use crate::vojo::app_config::Route;
use crate::vojo::app_config::ServiceConfig;
use crate::vojo::app_config::ServiceType;
use crate::vojo::app_config::StaticConifg;
use crate::vojo::authentication::AuthenticationStrategy;
use crate::vojo::health_check::HealthCheckType;
use crate::vojo::rate_limit::RatelimitStrategy;
use crate::vojo::route::AnomalyDetectionStatus;
use crate::vojo::route::BaseRoute;
use crate::vojo::route::HeaderValueMappingType;
use crate::vojo::route::LoadbalancerStrategy;
use crate::vojo::route::{
    HeaderBasedRoute, PollBaseRoute, PollRoute, RandomBaseRoute, RandomRoute, WeightBasedRoute,
    WeightRoute,
};
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;

use uuid::Uuid;

use super::route::HeaderRoute;
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiServiceVistor {
    pub listen_port: i32,
    #[serde(default = "new_uuid")]
    pub api_service_id: String,
    pub service_config: ServiceConfigVistor,
}
pub fn new_uuid() -> String {
    let id = Uuid::new_v4();
    id.to_string()
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceConfigVistor {
    pub server_type: ServiceType,
    pub cert_str: Option<String>,
    pub key_str: Option<String>,
    pub routes: Vec<RouteVistor>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteVistor {
    #[serde(default = "new_uuid")]
    pub route_id: String,
    pub host_name: Option<String>,
    pub matcher: Option<Matcher>,
    pub allow_deny_list: Option<Vec<AllowDenyObject>>,
    pub authentication: Option<Box<dyn AuthenticationStrategy>>,
    pub anomaly_detection: Option<AnomalyDetectionType>,
    #[serde(skip_serializing, skip_deserializing)]
    pub liveness_status: LivenessStatus,
    pub rewrite_headers: Option<HashMap<String, String>>,
    pub liveness_config: Option<LivenessConfig>,
    pub health_check: Option<HealthCheckType>,
    pub ratelimit: Option<Box<dyn RatelimitStrategy>>,
    pub route_cluster: LoadbalancerStrategyVistor,
}

impl RouteVistor {
    async fn from(route: Route) -> Result<RouteVistor, anyhow::Error> {
        let load = from_loadbalancer_strategy(route.route_cluster).await;
        let liveness_status = route.liveness_status.read().await;
        Ok(RouteVistor {
            route_id: route.route_id,
            host_name: route.host_name,
            matcher: route.matcher,
            rewrite_headers: route.rewrite_headers,
            allow_deny_list: route.allow_deny_list,
            authentication: route.authentication,
            anomaly_detection: route.anomaly_detection,
            liveness_status: liveness_status.clone(),
            liveness_config: route.liveness_config,
            health_check: route.health_check,
            ratelimit: route.ratelimit,
            route_cluster: load,
        })
    }
}
impl ServiceConfigVistor {
    pub async fn from(service_config: ServiceConfig) -> Result<Self, anyhow::Error> {
        let mut routes = vec![];
        for item in service_config.routes {
            routes.push(RouteVistor::from(item).await?)
        }
        Ok(ServiceConfigVistor {
            server_type: service_config.server_type,
            cert_str: service_config.cert_str,
            key_str: service_config.key_str,
            routes,
        })
    }
}
impl ApiServiceVistor {
    pub async fn from(api_service: ApiService) -> Result<Self, anyhow::Error> {
        let api_service_config = ServiceConfigVistor::from(api_service.service_config).await?;
        Ok(ApiServiceVistor {
            listen_port: api_service.listen_port,
            api_service_id: api_service.api_service_id,
            service_config: api_service_config,
        })
    }
}
pub async fn from_api_service_vistor(
    api_service_vistors: Vec<ApiServiceVistor>,
) -> Result<Vec<ApiService>, anyhow::Error> {
    let mut result = vec![];
    for item in api_service_vistors {
        result.push(ApiService::from(item).await?);
    }
    Ok(result)
}
pub async fn from_api_service(
    api_service_vistors: Vec<ApiService>,
) -> Result<Vec<ApiServiceVistor>, anyhow::Error> {
    let mut result = vec![];
    for item in api_service_vistors {
        result.push(ApiServiceVistor::from(item).await?);
    }
    Ok(result)
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfigVistor {
    pub static_config: StaticConifg,
    pub api_service_config: Vec<ApiServiceVistor>,
}
impl AppConfigVistor {
    pub async fn from(app_config: AppConfig) -> Result<Self, anyhow::Error> {
        let api_service_config = from_api_service(app_config.api_service_config).await?;
        Ok(AppConfigVistor {
            static_config: app_config.static_config.clone(),
            api_service_config,
        })
    }
}
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LoadbalancerStrategyVistor {
    PollRoute(PollRouteVistor),
    HeaderBasedRoute(HeaderBasedRouteVistor),
    RandomRoute(RandomRouteVistor),
    WeightBasedRoute(WeightBasedRouteVistor),
}
impl Default for LoadbalancerStrategyVistor {
    fn default() -> Self {
        LoadbalancerStrategyVistor::RandomRoute(Default::default())
    }
}
impl LoadbalancerStrategyVistor {
    pub fn get_routes_len(self) -> usize {
        match self {
            LoadbalancerStrategyVistor::PollRoute(poll_route_visitor) => {
                poll_route_visitor.routes.len()
            }
            LoadbalancerStrategyVistor::HeaderBasedRoute(header_based_route_vistor) => {
                header_based_route_vistor.routes.len()
            }
            LoadbalancerStrategyVistor::RandomRoute(random_base_route_vistor) => {
                random_base_route_vistor.routes.len()
            }
            LoadbalancerStrategyVistor::WeightBasedRoute(weight_based_route_vistor) => {
                weight_based_route_vistor.routes.len()
            }
        }
    }
}
pub fn from_loadbalancer_strategy_vistor(
    loadbalancer_strategy_vistor: LoadbalancerStrategyVistor,
) -> LoadbalancerStrategy {
    match loadbalancer_strategy_vistor {
        LoadbalancerStrategyVistor::PollRoute(poll_route_visitor) => {
            LoadbalancerStrategy::PollRoute(PollRoute::from(poll_route_visitor))
        }
        LoadbalancerStrategyVistor::HeaderBasedRoute(header_based_route_vistor) => {
            LoadbalancerStrategy::HeaderBased(HeaderBasedRoute::from(header_based_route_vistor))
        }
        LoadbalancerStrategyVistor::RandomRoute(random_base_route_vistor) => {
            LoadbalancerStrategy::Random(RandomRoute::from(random_base_route_vistor))
        }
        LoadbalancerStrategyVistor::WeightBasedRoute(weight_based_route_vistor) => {
            LoadbalancerStrategy::WeightBased(WeightBasedRoute::from(weight_based_route_vistor))
        }
    }
}
pub async fn from_loadbalancer_strategy(
    loadbalancer_strategy: LoadbalancerStrategy,
) -> LoadbalancerStrategyVistor {
    match loadbalancer_strategy {
        LoadbalancerStrategy::PollRoute(poll_route) => {
            LoadbalancerStrategyVistor::PollRoute(PollRouteVistor::from(poll_route).await)
        }
        LoadbalancerStrategy::HeaderBased(header_based_route) => {
            LoadbalancerStrategyVistor::HeaderBasedRoute(
                HeaderBasedRouteVistor::from(header_based_route).await,
            )
        }
        LoadbalancerStrategy::Random(random_base_route) => LoadbalancerStrategyVistor::RandomRoute(
            RandomRouteVistor::from(random_base_route).await,
        ),
        LoadbalancerStrategy::WeightBased(weight_based_route) => {
            LoadbalancerStrategyVistor::WeightBasedRoute(
                WeightBasedRouteVistor::from(weight_based_route).await,
            )
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]

pub struct PollRouteVistor {
    #[serde(skip_serializing, skip_deserializing)]
    pub current_index: i32,
    pub routes: Vec<PollBaseRouteVistor>,
}
impl PollRouteVistor {
    pub async fn from(poll_route: PollRoute) -> Self {
        let mut res = vec![];
        for item in poll_route.routes {
            let current = PollBaseRouteVistor::from(item).await;
            res.push(current);
        }
        PollRouteVistor {
            current_index: poll_route.current_index.load(Ordering::SeqCst) as i32,
            routes: res,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PollBaseRouteVistor {
    pub base_route: BaseRouteVistor,
}
impl PollBaseRouteVistor {
    pub async fn from(poll_route: PollBaseRoute) -> Self {
        PollBaseRouteVistor {
            base_route: BaseRouteVistor::from(poll_route.base_route).await,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BaseRouteVistor {
    pub endpoint: String,
    pub try_file: Option<String>,
    pub is_alive: Option<bool>,
    #[serde(skip_serializing, skip_deserializing)]
    pub anomaly_detection_status: AnomalyDetectionStatus,
}
impl BaseRouteVistor {
    pub async fn from(base_route: BaseRoute) -> Self {
        let is_alive = base_route.is_alive.read().await;
        let anomaly_detection_status = base_route.anomaly_detection_status.read().await;
        BaseRouteVistor {
            endpoint: base_route.endpoint,
            try_file: base_route.try_file,
            is_alive: *is_alive,
            anomaly_detection_status: anomaly_detection_status.clone(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeaderRouteVistor {
    pub base_route: BaseRouteVistor,
    pub header_key: String,
    pub header_value_mapping_type: HeaderValueMappingType,
}
impl HeaderRouteVistor {
    pub async fn new_list(header_based_routes: Vec<HeaderRoute>) -> Vec<HeaderRouteVistor> {
        let mut res = vec![];
        for item in header_based_routes {
            let base_route_visitor = BaseRouteVistor::from(item.base_route).await;
            let current = HeaderRouteVistor {
                base_route: base_route_visitor,
                header_key: item.header_key,
                header_value_mapping_type: item.header_value_mapping_type,
            };
            res.push(current);
        }
        res
    }
}
fn default_weight() -> i32 {
    100
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeaderBasedRouteVistor {
    pub routes: Vec<HeaderRouteVistor>,
}
impl HeaderBasedRouteVistor {
    pub async fn from(header_basedroute: HeaderBasedRoute) -> Self {
        HeaderBasedRouteVistor {
            routes: HeaderRouteVistor::new_list(header_basedroute.routes).await,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RandomBaseRouteVistor {
    pub base_route: BaseRouteVistor,
}
impl RandomBaseRouteVistor {
    pub async fn new_list(random_routes: Vec<RandomBaseRoute>) -> Vec<RandomBaseRouteVistor> {
        let mut result = vec![];
        for item in random_routes {
            result.push(RandomBaseRouteVistor {
                base_route: BaseRouteVistor::from(item.base_route).await,
            })
        }
        result
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightRouteVistor {
    pub base_route: BaseRouteVistor,
    #[serde(default = "default_weight")]
    pub weight: i32,
    #[serde(skip_serializing, skip_deserializing)]
    pub index: i32,
}
impl WeightRouteVistor {
    pub async fn new_list(weight_based_routes: Vec<WeightRoute>) -> Vec<WeightRouteVistor> {
        let mut res = vec![];
        for item in weight_based_routes {
            let base_routes = BaseRouteVistor::from(item.base_route).await;
            res.push(WeightRouteVistor {
                base_route: base_routes,
                weight: item.weight,
                index: item.index.load(Ordering::SeqCst) as i32,
            });
        }
        res
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightBasedRouteVistor {
    pub routes: Vec<WeightRouteVistor>,
}
impl WeightBasedRouteVistor {
    pub async fn from(weight_based_route: WeightBasedRoute) -> Self {
        let list = weight_based_route.routes.read().await;
        WeightBasedRouteVistor {
            routes: WeightRouteVistor::new_list(list.clone()).await,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RandomRouteVistor {
    pub routes: Vec<RandomBaseRouteVistor>,
}
impl RandomRouteVistor {
    pub async fn from(random_route: RandomRoute) -> Self {
        RandomRouteVistor {
            routes: RandomBaseRouteVistor::new_list(random_route.routes).await,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::uuid::get_uuid;
    use crate::vojo::anomaly_detection::AnomalyDetectionType;
    use crate::vojo::anomaly_detection::BaseAnomalyDetectionParam;
    use crate::vojo::anomaly_detection::HttpAnomalyDetectionParam;

    use crate::vojo::health_check::BaseHealthCheckParam;
    use crate::vojo::health_check::HttpHealthCheckParam;
    use crate::vojo::route::HeaderValueMappingType;
    use crate::vojo::route::RegexMatch;
    use std::sync::atomic::AtomicIsize;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    #[tokio::test]
    async fn test_from_api_service_vistor_ok1() {
        let route = RouteVistor {
            host_name: None,
            route_id: get_uuid(),
            route_cluster: LoadbalancerStrategyVistor::WeightBasedRoute(WeightBasedRouteVistor {
                routes: vec![WeightRouteVistor {
                    base_route: BaseRouteVistor {
                        endpoint: String::from("/"),
                        try_file: None,
                        is_alive: None,
                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    },
                    index: 0,
                    weight: 100,
                }],
            }),
            health_check: Some(HealthCheckType::HttpGet(HttpHealthCheckParam {
                base_health_check_param: BaseHealthCheckParam {
                    timeout: 10,
                    interval: 10,
                },
                path: String::from("value"),
            })),
            liveness_status: LivenessStatus {
                current_liveness_count: 0,
            },
            anomaly_detection: Some(AnomalyDetectionType::Http(HttpAnomalyDetectionParam {
                consecutive_5xx: 23,
                base_anomaly_detection_param: BaseAnomalyDetectionParam {
                    ejection_second: 23,
                },
            })),
            rewrite_headers: None,
            allow_deny_list: None,
            authentication: None,
            liveness_config: Some(LivenessConfig {
                min_liveness_count: 32,
            }),

            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service_vistor = ApiServiceVistor {
            api_service_id: get_uuid(),
            listen_port: 4486,
            service_config: ServiceConfigVistor {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),
                key_str: Default::default(),
            },
        };
        let api_services = vec![api_service_vistor];
        let result = from_api_service_vistor(api_services).await;
        assert!(result.is_ok())
    }
    #[tokio::test]
    async fn test_from_api_service_vistor_ok2() {
        let route = RouteVistor {
            host_name: None,
            route_id: get_uuid(),
            route_cluster: LoadbalancerStrategyVistor::HeaderBasedRoute(HeaderBasedRouteVistor {
                routes: vec![HeaderRouteVistor {
                    header_key: String::from("s"),
                    header_value_mapping_type: HeaderValueMappingType::Regex(RegexMatch {
                        value: String::from("^100*"),
                    }),
                    base_route: BaseRouteVistor {
                        endpoint: String::from("/"),
                        try_file: None,
                        is_alive: None,
                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    },
                }],
            }),
            health_check: Some(HealthCheckType::HttpGet(HttpHealthCheckParam {
                base_health_check_param: BaseHealthCheckParam {
                    timeout: 10,
                    interval: 10,
                },
                path: String::from("value"),
            })),
            liveness_status: LivenessStatus {
                current_liveness_count: 0,
            },
            anomaly_detection: Some(AnomalyDetectionType::Http(HttpAnomalyDetectionParam {
                consecutive_5xx: 23,
                base_anomaly_detection_param: BaseAnomalyDetectionParam {
                    ejection_second: 23,
                },
            })),
            rewrite_headers: None,

            allow_deny_list: None,
            authentication: None,
            liveness_config: Some(LivenessConfig {
                min_liveness_count: 32,
            }),

            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service_vistor = ApiServiceVistor {
            api_service_id: get_uuid(),
            listen_port: 4486,
            service_config: ServiceConfigVistor {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),
                key_str: Default::default(),
            },
        };
        let api_services = vec![api_service_vistor];
        let result = from_api_service_vistor(api_services).await;
        assert!(result.is_ok())
    }
    #[tokio::test]
    async fn test_from_api_service_ok1() {
        let route = Route {
            host_name: None,
            route_id: get_uuid(),
            route_cluster: LoadbalancerStrategy::WeightBased(WeightBasedRoute {
                routes: Arc::new(RwLock::new(vec![WeightRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                        is_alive: Arc::new(RwLock::new(None)),
                        anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        })),
                    },
                    index: Arc::new(AtomicIsize::new(0)),
                    weight: 100,
                }])),
            }),
            health_check: Some(HealthCheckType::HttpGet(HttpHealthCheckParam {
                base_health_check_param: BaseHealthCheckParam {
                    timeout: 10,
                    interval: 10,
                },
                path: String::from("value"),
            })),
            liveness_status: Arc::new(RwLock::new(LivenessStatus {
                current_liveness_count: 0,
            })),
            anomaly_detection: Some(AnomalyDetectionType::Http(HttpAnomalyDetectionParam {
                consecutive_5xx: 23,
                base_anomaly_detection_param: BaseAnomalyDetectionParam {
                    ejection_second: 23,
                },
            })),
            rewrite_headers: None,

            allow_deny_list: None,
            authentication: None,
            liveness_config: Some(LivenessConfig {
                min_liveness_count: 32,
            }),

            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
            api_service_id: get_uuid(),
            listen_port: 4486,
            service_config: ServiceConfig {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),
                key_str: Default::default(),
            },
        };
        let api_services = vec![api_service];
        let result = from_api_service(api_services).await;
        assert!(result.is_ok())
    }
    #[tokio::test]
    async fn test_from_api_service_ok2() {
        let route = Route {
            host_name: None,
            route_id: get_uuid(),
            route_cluster: LoadbalancerStrategy::WeightBased(WeightBasedRoute {
                routes: Arc::new(RwLock::new(vec![WeightRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                        is_alive: Arc::new(RwLock::new(None)),
                        anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        })),
                    },
                    index: Arc::new(AtomicIsize::new(0)),
                    weight: 100,
                }])),
            }),
            health_check: Some(HealthCheckType::HttpGet(HttpHealthCheckParam {
                base_health_check_param: BaseHealthCheckParam {
                    timeout: 10,
                    interval: 10,
                },
                path: String::from("value"),
            })),
            liveness_status: Arc::new(RwLock::new(LivenessStatus {
                current_liveness_count: 0,
            })),
            anomaly_detection: Some(AnomalyDetectionType::Http(HttpAnomalyDetectionParam {
                consecutive_5xx: 23,
                base_anomaly_detection_param: BaseAnomalyDetectionParam {
                    ejection_second: 23,
                },
            })),
            rewrite_headers: None,

            allow_deny_list: None,
            authentication: None,
            liveness_config: Some(LivenessConfig {
                min_liveness_count: 32,
            }),

            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
            api_service_id: get_uuid(),
            listen_port: 4486,
            service_config: ServiceConfig {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),
                key_str: Default::default(),
            },
        };
        let api_services = vec![api_service];
        let result = from_api_service(api_services).await;
        assert!(result.is_ok())
    }
}
