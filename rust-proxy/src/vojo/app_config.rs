use super::allow_deny_ip::AllowResult;
use super::app_config_vistor::ApiServiceVistor;
use super::app_config_vistor::ServiceConfigVistor;
use crate::vojo::allow_deny_ip::AllowDenyObject;
use crate::vojo::anomaly_detection::AnomalyDetectionType;
use crate::vojo::app_config_vistor::from_loadbalancer_strategy_vistor;
use crate::vojo::app_config_vistor::RouteVistor;
use crate::vojo::authentication::AuthenticationStrategy;
use crate::vojo::health_check::HealthCheckType;
use crate::vojo::rate_limit::RatelimitStrategy;
use crate::vojo::route::LoadbalancerStrategy;
use http::HeaderMap;
use http::HeaderValue;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Matcher {
    pub prefix: String,
    pub prefix_rewrite: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LivenessConfig {
    pub min_liveness_count: i32,
}
#[derive(Debug, Serialize, Clone, Deserialize, Default)]
pub struct LivenessStatus {
    pub current_liveness_count: i32,
}
#[derive(Debug, Clone)]
pub struct Route {
    pub route_id: String,
    pub host_name: Option<String>,
    pub matcher: Option<Matcher>,
    pub allow_deny_list: Option<Vec<AllowDenyObject>>,
    pub authentication: Option<Box<dyn AuthenticationStrategy>>,
    pub anomaly_detection: Option<AnomalyDetectionType>,
    pub liveness_status: Arc<RwLock<LivenessStatus>>,
    pub liveness_config: Option<LivenessConfig>,
    pub health_check: Option<HealthCheckType>,
    pub ratelimit: Option<Box<dyn RatelimitStrategy>>,
    pub route_cluster: LoadbalancerStrategy,
}
impl Route {
    pub async fn from(route_vistor: RouteVistor) -> Result<Route, anyhow::Error> {
        let cloned_cluster = route_vistor.route_cluster.clone();
        let new_matcher = route_vistor.matcher.clone().map(|mut item| {
            let src_prefix = item.prefix.clone();
            if !src_prefix.ends_with('/') {
                let src_prefix_len = item.prefix.len();
                item.prefix.insert(src_prefix_len, '/');
            }
            if !src_prefix.starts_with('/') {
                item.prefix.insert(0, '/')
            }
            item
        });

        let count = cloned_cluster.get_routes_len() as i32;

        Ok(Route {
            route_id: route_vistor.route_id,
            host_name: route_vistor.host_name,
            matcher: new_matcher,
            allow_deny_list: route_vistor.allow_deny_list,
            authentication: route_vistor.authentication,
            anomaly_detection: route_vistor.anomaly_detection,
            liveness_status: Arc::new(RwLock::new(LivenessStatus {
                current_liveness_count: count,
            })),
            liveness_config: route_vistor.liveness_config,
            health_check: route_vistor.health_check,
            ratelimit: route_vistor.ratelimit,
            route_cluster: from_loadbalancer_strategy_vistor(route_vistor.route_cluster),
        })
    }
}

impl Route {
    pub fn is_matched(
        &self,
        path: &str,
        headers_option: Option<HeaderMap<HeaderValue>>,
    ) -> Result<Option<String>, anyhow::Error> {
        let match_prefix = self
            .clone()
            .matcher
            .ok_or("The matcher counld not be none for http")
            .map_err(|err| anyhow!(err))?
            .prefix;

        // let re = Regex::new(match_prefix.as_str()).unwrap();
        // let match_res = re.captures(path);
        let match_res = path.strip_prefix(match_prefix.as_str());
        if match_res.is_none() {
            return Ok(None);
        }
        if let Some(real_host_name) = &self.host_name {
            if headers_option.is_none() {
                return Ok(None);
            }
            let header_map = headers_option.unwrap();
            let host_option = header_map.get("Host");
            if host_option.is_none() {
                return Ok(None);
            }
            let host_result = host_option.unwrap().to_str();
            if host_result.is_err() {
                return Ok(None);
            }
            let host_name_regex = Regex::new(real_host_name.as_str())?;
            return host_name_regex
                .captures(host_result.unwrap())
                .map_or(Ok(None), |_| Ok(Some(String::from(match_res.unwrap()))));
        }
        Ok(Some(String::from(match_res.unwrap())))
    }
    pub async fn is_allowed(
        &self,
        ip: String,
        headers_option: Option<HeaderMap<HeaderValue>>,
    ) -> Result<bool, anyhow::Error> {
        let mut is_allowed = ip_is_allowed(self.allow_deny_list.clone(), ip.clone())?;
        if !is_allowed {
            return Ok(is_allowed);
        }
        if let (Some(header_map), Some(mut authentication_strategy)) =
            (headers_option.clone(), self.authentication.clone())
        {
            is_allowed = authentication_strategy.check_authentication(header_map)?;
            if !is_allowed {
                return Ok(is_allowed);
            }
        }
        if let (Some(header_map), Some(mut ratelimit_strategy)) =
            (headers_option, self.ratelimit.clone())
        {
            is_allowed = !ratelimit_strategy.should_limit(header_map, ip).await?;
        }
        Ok(is_allowed)
    }
}
pub fn ip_is_allowed(
    allow_deny_list: Option<Vec<AllowDenyObject>>,
    ip: String,
) -> Result<bool, anyhow::Error> {
    if allow_deny_list.is_none() || allow_deny_list.clone().unwrap().is_empty() {
        return Ok(true);
    }
    let allow_deny_list = allow_deny_list.unwrap();
    // let iter = allow_deny_list.iter();

    for item in allow_deny_list {
        let is_allow = item.is_allow(ip.clone());
        match is_allow {
            Ok(AllowResult::Allow) => {
                return Ok(true);
            }
            Ok(AllowResult::Deny) => {
                return Ok(false);
            }
            Ok(AllowResult::Notmapping) => {
                continue;
            }
            Err(err) => {
                return Err(anyhow!(err.to_string()));
            }
        }
    }

    Ok(true)
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, strum_macros::Display)]
pub enum ServiceType {
    #[default]
    Http,
    Https,
    Tcp,
    Websocket,
    WebsocketTls,
    Grpc,
    GrpcTls,
}
#[derive(Debug, Clone, Default)]
pub struct ServiceConfig {
    pub server_type: ServiceType,
    pub cert_str: Option<String>,
    pub key_str: Option<String>,
    pub routes: Vec<Route>,
}
impl ServiceConfig {
    pub async fn from(service_config_vistor: ServiceConfigVistor) -> Result<Self, anyhow::Error> {
        let mut routes = vec![];
        for item in service_config_vistor.routes {
            routes.push(Route::from(item).await?)
        }
        Ok(ServiceConfig {
            server_type: service_config_vistor.server_type,
            cert_str: service_config_vistor.cert_str,
            key_str: service_config_vistor.key_str,
            routes,
        })
    }
}
#[derive(Debug, Clone, Default)]
pub struct ApiService {
    pub listen_port: i32,
    pub api_service_id: String,
    pub service_config: ServiceConfig,
}
impl ApiService {
    pub async fn from(api_service_vistor: ApiServiceVistor) -> Result<Self, anyhow::Error> {
        let api_service_config = ServiceConfig::from(api_service_vistor.service_config).await?;
        Ok(ApiService {
            listen_port: api_service_vistor.listen_port,
            api_service_id: api_service_vistor.api_service_id,
            service_config: api_service_config,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StaticConifg {
    pub access_log: Option<String>,
    pub database_url: Option<String>,
    pub admin_port: String,
    pub config_file_path: Option<String>,
}
#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    pub static_config: StaticConifg,
    pub api_service_config: Vec<ApiService>,
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::uuid::get_uuid;
    use crate::vojo::anomaly_detection::BaseAnomalyDetectionParam;
    use crate::vojo::anomaly_detection::HttpAnomalyDetectionParam;
    use crate::vojo::app_config_vistor::BaseRouteVistor;
    use crate::vojo::app_config_vistor::LoadbalancerStrategyVistor;
    use crate::vojo::app_config_vistor::PollBaseRouteVistor;
    use crate::vojo::app_config_vistor::PollRouteVistor;
    use crate::vojo::app_config_vistor::RandomBaseRouteVistor;
    use crate::vojo::app_config_vistor::RandomRouteVistor;
    use crate::vojo::app_config_vistor::WeightBasedRouteVistor;
    use crate::vojo::app_config_vistor::WeightRouteVistor;
    use crate::vojo::authentication::ApiKeyAuth;
    use crate::vojo::authentication::AuthenticationStrategy;
    use crate::vojo::authentication::BasicAuth;
    use crate::vojo::health_check::{BaseHealthCheckParam, HttpHealthCheckParam};
    use crate::vojo::rate_limit::*;
    use crate::vojo::route::AnomalyDetectionStatus;
    use crate::vojo::route::BaseRoute;

    use crate::vojo::route::WeightBasedRoute;
    use crate::vojo::route::WeightRoute;
    use dashmap::DashMap;
    use std::sync::atomic::AtomicIsize;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::SystemTime;
    use tokio::sync::RwLock;
    fn create_new_route_with_host_name(host_name: Option<String>) -> Route {
        Route {
            host_name,
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
            liveness_status: Arc::new(RwLock::new(LivenessStatus {
                current_liveness_count: 0,
            })),
            anomaly_detection: None,
            health_check: None,
            allow_deny_list: None,
            authentication: None,
            liveness_config: None,

            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("/"),
                prefix_rewrite: String::from("ssss"),
            }),
        }
    }
    #[test]
    fn test_host_name_is_none_ok1() {
        let route = create_new_route_with_host_name(None);
        let mut headermap = HeaderMap::new();
        headermap.insert("x-client", "Basic bHNrOjEyMzQ=".parse().unwrap());
        let allow_result = route.is_matched("/test", Some(headermap));
        assert!(allow_result.is_ok());
        assert!(allow_result.unwrap().is_some());
    }

    #[test]
    fn test_host_name_is_some_ok2() {
        let route = create_new_route_with_host_name(Some(String::from("www.test.com")));
        let mut headermap = HeaderMap::new();
        headermap.insert("x-client", "Basic bHNrOjEyMzQ=".parse().unwrap());
        let allow_result = route.is_matched("/test", Some(headermap));
        assert!(allow_result.is_ok());
        assert!(allow_result.unwrap().is_none());
    }
    #[test]
    fn test_host_name_is_some_ok3() {
        let route = create_new_route_with_host_name(Some(String::from("www.test.com")));
        let mut headermap = HeaderMap::new();
        headermap.insert("Host", "Basic bHNrOjEyMzQ=".parse().unwrap());
        let allow_result = route.is_matched("/test", Some(headermap));
        assert!(allow_result.is_ok());
        assert!(allow_result.unwrap().is_none());
    }
    #[test]
    fn test_host_name_is_some_ok4() {
        let route = create_new_route_with_host_name(Some(String::from("www.test.com")));
        let mut headermap = HeaderMap::new();
        headermap.insert("Host", "www.test.com".parse().unwrap());
        let allow_result = route.is_matched("/test", Some(headermap));
        assert!(allow_result.is_ok());
        assert!(allow_result.unwrap().is_some());
    }
    #[test]
    fn test_serde_output_health_check() {
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
        let api_service = ApiServiceVistor {
            api_service_id: get_uuid(),
            listen_port: 4486,
            service_config: ServiceConfigVistor {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),
                key_str: Default::default(),
            },
        };
        let t = vec![api_service];
        let yaml = serde_yaml::to_string(&t).unwrap();
        println!("{}", yaml);
    }

    #[test]
    fn test_serde_output_weight_based_route() {
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
            liveness_status: LivenessStatus {
                current_liveness_count: 0,
            },
            anomaly_detection: None,
            health_check: None,
            allow_deny_list: None,
            authentication: None,
            liveness_config: None,

            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiServiceVistor {
            api_service_id: get_uuid(),
            listen_port: 4486,
            service_config: ServiceConfigVistor {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),
                key_str: Default::default(),
            },
        };
        let t = vec![api_service];
        let yaml = serde_yaml::to_string(&t).unwrap();
        println!("{}", yaml);
    }

    #[test]
    fn test_serde_output_header_based_route() {
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
            liveness_status: LivenessStatus {
                current_liveness_count: 0,
            },
            anomaly_detection: None,
            health_check: None,
            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            liveness_config: None,

            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiServiceVistor {
            api_service_id: get_uuid(),
            listen_port: 4486,
            service_config: ServiceConfigVistor {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),
                key_str: Default::default(),
            },
        };
        let t = vec![api_service];
        let yaml = serde_yaml::to_string(&t).unwrap();
        println!("{}", yaml);
    }

    #[test]
    fn test_serde_output_random_route() {
        let route = RouteVistor {
            host_name: None,
            route_id: get_uuid(),
            route_cluster: LoadbalancerStrategyVistor::RandomRoute(RandomRouteVistor {
                routes: vec![
                    RandomBaseRouteVistor {
                        base_route: BaseRouteVistor {
                            endpoint: String::from("/"),
                            try_file: None,
                            is_alive: None,
                            anomaly_detection_status: AnomalyDetectionStatus {
                                consecutive_5xx: 100,
                            },
                        },
                    },
                    RandomBaseRouteVistor {
                        base_route: BaseRouteVistor {
                            endpoint: String::from("/"),
                            try_file: None,
                            is_alive: None,
                            anomaly_detection_status: AnomalyDetectionStatus {
                                consecutive_5xx: 100,
                            },
                        },
                    },
                ],
            }),
            liveness_config: None,
            liveness_status: LivenessStatus {
                current_liveness_count: 0,
            },
            anomaly_detection: None,
            allow_deny_list: None,
            authentication: None,
            health_check: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiServiceVistor {
            api_service_id: get_uuid(),
            listen_port: 4486,
            service_config: ServiceConfigVistor {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),

                key_str: Default::default(),
            },
        };
        let t = vec![api_service];
        let yaml = serde_yaml::to_string(&t).unwrap();
        println!("{}", yaml);
    }
    #[test]
    fn test_serde_output_poll_route() {
        let route = RouteVistor {
            host_name: None,
            route_id: get_uuid(),
            route_cluster: LoadbalancerStrategyVistor::PollRoute(PollRouteVistor {
                routes: vec![PollBaseRouteVistor {
                    base_route: BaseRouteVistor {
                        endpoint: String::from("/"),
                        try_file: None,
                        is_alive: None,
                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    },
                }],
                // lock: Default::default(),
                current_index: Default::default(),
            }),
            liveness_config: None,
            liveness_status: LivenessStatus {
                current_liveness_count: 0,
            },
            anomaly_detection: None,
            health_check: None,
            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiServiceVistor {
            api_service_id: get_uuid(),

            listen_port: 4486,
            service_config: ServiceConfigVistor {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),
                key_str: Default::default(),
            },
        };
        let t = vec![api_service];
        let yaml = serde_yaml::to_string(&t).unwrap();
        println!("{}", yaml);
    }

    #[test]
    fn test_serde_output_basic_auth() {
        let basic_auth: Box<dyn AuthenticationStrategy> = Box::new(BasicAuth {
            credentials: String::from("lsk:123456"),
        });
        let route = RouteVistor {
            host_name: None,
            route_id: get_uuid(),
            route_cluster: LoadbalancerStrategyVistor::PollRoute(PollRouteVistor {
                routes: vec![PollBaseRouteVistor {
                    base_route: BaseRouteVistor {
                        endpoint: String::from("/"),
                        try_file: None,
                        is_alive: None,
                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    },
                }],
                // lock: Default::default(),
                current_index: Default::default(),
            }),
            anomaly_detection: None,
            health_check: None,
            allow_deny_list: None,
            liveness_config: None,
            liveness_status: LivenessStatus {
                current_liveness_count: 0,
            },
            authentication: Some(basic_auth),
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiServiceVistor {
            listen_port: 4486,
            api_service_id: get_uuid(),
            service_config: ServiceConfigVistor {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),
                key_str: Default::default(),
            },
        };
        let t = vec![api_service];
        let yaml = serde_yaml::to_string(&t).unwrap();
        println!("{}", yaml);
    }
    #[test]
    fn test_serde_output_api_key_auth() {
        let api_key_auth: Box<dyn AuthenticationStrategy> = Box::new(ApiKeyAuth {
            key: String::from("api_key"),
            value: String::from("test"),
        });
        let route = RouteVistor {
            host_name: None,
            route_id: get_uuid(),
            route_cluster: LoadbalancerStrategyVistor::PollRoute(PollRouteVistor {
                routes: vec![PollBaseRouteVistor {
                    base_route: BaseRouteVistor {
                        endpoint: String::from("/"),
                        try_file: None,
                        is_alive: None,
                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    },
                }],
                // lock: Default::default(),
                current_index: Default::default(),
            }),
            anomaly_detection: None,
            health_check: None,
            allow_deny_list: None,
            liveness_config: None,
            liveness_status: LivenessStatus {
                current_liveness_count: 0,
            },
            ratelimit: None,
            authentication: Some(api_key_auth),
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiServiceVistor {
            api_service_id: get_uuid(),
            listen_port: 4486,
            service_config: ServiceConfigVistor {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),
                key_str: Default::default(),
            },
        };
        let t = vec![api_service];
        let yaml = serde_yaml::to_string(&t).unwrap();
        println!("{}", yaml);
    }
    #[test]
    fn test_serde_output_token_bucket_ratelimit() {
        let token_bucket_ratelimit = TokenBucketRateLimit {
            rate_per_unit: 3,
            capacity: 10000,
            unit: TimeUnit::Second,
            limit_location: LimitLocation::IP(IPBasedRatelimit {
                value: String::from("192.168.0.0"),
            }),
            current_count: Arc::new(RwLock::new(AtomicIsize::new(3))),
            lock: Arc::new(Mutex::new(0)),
            last_update_time: Arc::new(RwLock::new(SystemTime::now())),
        };
        let ratelimit: Box<dyn RatelimitStrategy> = Box::new(token_bucket_ratelimit);
        let route = RouteVistor {
            host_name: None,
            route_id: get_uuid(),
            route_cluster: LoadbalancerStrategyVistor::PollRoute(PollRouteVistor {
                routes: vec![PollBaseRouteVistor {
                    base_route: BaseRouteVistor {
                        endpoint: String::from("/"),
                        try_file: None,
                        is_alive: None,
                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    },
                }],
                // lock: Default::default(),
                current_index: Default::default(),
            }),
            liveness_status: LivenessStatus {
                current_liveness_count: 0,
            },
            anomaly_detection: None,
            health_check: None,
            allow_deny_list: None,
            liveness_config: None,

            authentication: None,
            ratelimit: Some(ratelimit),
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiServiceVistor {
            api_service_id: get_uuid(),
            listen_port: 4486,
            service_config: ServiceConfigVistor {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),
                key_str: Default::default(),
            },
        };
        let t = vec![api_service];
        let yaml = serde_yaml::to_string(&t).unwrap();
        println!("{}", yaml);
    }
    #[test]
    fn test_serde_output_fixedwindow_ratelimit() {
        let fixed_window_ratelimit = FixedWindowRateLimit {
            rate_per_unit: 3,
            unit: TimeUnit::Minute,
            limit_location: LimitLocation::IP(IPBasedRatelimit {
                value: String::from("192.168.0.0"),
            }),
            count_map: Arc::new(DashMap::new()),
            lock: Arc::new(Mutex::new(0)),
        };
        let ratelimit: Box<dyn RatelimitStrategy> = Box::new(fixed_window_ratelimit);
        let route = RouteVistor {
            host_name: None,
            route_id: get_uuid(),
            route_cluster: LoadbalancerStrategyVistor::PollRoute(PollRouteVistor {
                routes: vec![PollBaseRouteVistor {
                    base_route: BaseRouteVistor {
                        endpoint: String::from("/"),
                        try_file: None,
                        is_alive: None,
                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    },
                }],
                // lock: Default::default(),
                current_index: Default::default(),
            }),
            liveness_status: LivenessStatus {
                current_liveness_count: 0,
            },
            anomaly_detection: None,
            health_check: None,
            allow_deny_list: None,
            authentication: None,
            liveness_config: None,

            ratelimit: Some(ratelimit),
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiServiceVistor {
            api_service_id: get_uuid(),
            listen_port: 4486,
            service_config: ServiceConfigVistor {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),
                key_str: Default::default(),
            },
        };
        let t = vec![api_service];
        let yaml = serde_yaml::to_string(&t).unwrap();
        println!("{}", yaml);
    }

    #[test]
    fn test_serde_output_allow_deny_list() {
        let allow_object = AllowDenyObject {
            limit_type: crate::vojo::allow_deny_ip::AllowType::Allow,
            value: Some(String::from("sss")),
        };
        let route = RouteVistor {
            host_name: None,
            route_id: get_uuid(),
            route_cluster: LoadbalancerStrategyVistor::PollRoute(PollRouteVistor {
                routes: vec![PollBaseRouteVistor {
                    base_route: BaseRouteVistor {
                        endpoint: String::from("/"),
                        try_file: None,
                        is_alive: None,
                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    },
                }],
                // lock: Default::default(),
                current_index: Default::default(),
            }),
            anomaly_detection: None,
            health_check: None,
            allow_deny_list: Some(vec![allow_object]),
            authentication: None,
            liveness_config: None,
            liveness_status: LivenessStatus {
                current_liveness_count: 0,
            },
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiServiceVistor {
            api_service_id: get_uuid(),
            listen_port: 4486,
            service_config: ServiceConfigVistor {
                routes: vec![route],
                server_type: Default::default(),
                cert_str: Default::default(),
                key_str: Default::default(),
            },
        };
        let t = vec![api_service];
        let yaml = serde_yaml::to_string(&t).unwrap();
        println!("{}", yaml);
    }
    #[test]
    fn test_regex() {
        // let re = Regex::new("/api/test/book").unwrap();
        // let match_res = re.captures("/api");
        // assert_eq!(match_res.is_some(), true);
        let src_path1 = "/api/test/book";
        let dst1 = src_path1.strip_prefix("/api");
        assert!(dst1.is_some());
        assert_eq!(dst1.unwrap(), "/test/book");

        let src_path2 = "/api/test/book";
        let dst2 = src_path2.strip_prefix("api");
        assert!(dst2.is_none());

        let src_path3 = "/api/test/book";
        let dst3 = src_path3.strip_prefix("/api/");
        assert!(dst3.is_some());
        assert_eq!(dst3.unwrap(), "test/book");
    }
}
