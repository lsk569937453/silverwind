use super::allow_deny_ip::AllowResult;
use crate::vojo::allow_deny_ip::AllowDenyObject;
use crate::vojo::authentication::AuthenticationStrategy;
use crate::vojo::health_check::HealthCheckType;
use crate::vojo::rate_limit::RatelimitStrategy;
use crate::vojo::route::LoadbalancerStrategy;
use http::HeaderMap;
use http::HeaderValue;
use regex::Regex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Matcher {
    pub prefix: String,
    pub prefix_rewrite: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    #[serde(default = "new_uuid")]
    pub route_id: String,
    pub host_name: Option<String>,
    pub matcher: Option<Matcher>,
    pub allow_deny_list: Option<Vec<AllowDenyObject>>,
    pub authentication: Option<Box<dyn AuthenticationStrategy>>,
    pub health_check: Option<HealthCheckType>,
    pub ratelimit: Option<Box<dyn RatelimitStrategy>>,
    pub route_cluster: Box<dyn LoadbalancerStrategy>,
}
pub fn new_uuid() -> String {
    let id = Uuid::new_v4();
    id.to_string()
}
impl Route {
    pub fn is_matched(
        &self,
        path: &str,
        headers_option: Option<HeaderMap<HeaderValue>>,
    ) -> Result<bool, anyhow::Error> {
        let match_prefix = self
            .clone()
            .matcher
            .ok_or("The matcher counld not be none for http")
            .map_err(|err| anyhow!(err))?
            .prefix;

        let re = Regex::new(match_prefix.as_str()).unwrap();
        let match_res = re.captures(path);
        if match_res.is_none() {
            return Ok(false);
        }
        if let Some(real_host_name) = &self.host_name {
            if headers_option.is_none() {
                return Ok(false);
            }
            let header_map = headers_option.clone().unwrap();
            let host_option = header_map.get("Host");
            if host_option.is_none() {
                return Ok(false);
            }
            let host_result = host_option.unwrap().to_str();
            if host_result.is_err() {
                return Ok(false);
            }
            let host_name_regex = Regex::new(real_host_name.as_str())?;
            return host_name_regex
                .captures(host_result.unwrap())
                .map_or(Ok(false), |_| {
                    return Ok(true);
                });
        }
        Ok(true)
    }
    pub fn is_allowed(
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
            is_allowed = !ratelimit_strategy.should_limit(header_map, ip)?;
        }
        Ok(is_allowed)
    }
}
pub fn ip_is_allowed(
    allow_deny_list: Option<Vec<AllowDenyObject>>,
    ip: String,
) -> Result<bool, anyhow::Error> {
    if allow_deny_list == None || allow_deny_list.clone().unwrap().len() == 0 {
        return Ok(true);
    }
    let allow_deny_list = allow_deny_list.clone().unwrap();
    let iter = allow_deny_list.iter();

    for item in iter {
        let is_allow = item.is_allow(ip.clone());
        match is_allow {
            Ok(AllowResult::ALLOW) => {
                return Ok(true);
            }
            Ok(AllowResult::DENY) => {
                return Ok(false);
            }
            Ok(AllowResult::NOTMAPPING) => {
                break;
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
    HTTP,
    HTTPS,
    TCP,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServiceConfig {
    pub server_type: ServiceType,
    pub cert_str: Option<String>,
    pub key_str: Option<String>,
    pub routes: Vec<Route>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiService {
    pub listen_port: i32,
    #[serde(default = "new_uuid")]
    pub api_service_id: String,
    pub service_config: ServiceConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StaticConifg {
    pub access_log: Option<String>,
    pub database_url: Option<String>,
    pub admin_port: String,
    pub config_file_path: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub static_config: StaticConifg,
    pub api_service_config: Vec<ApiService>,
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vojo::authentication::ApiKeyAuth;
    use crate::vojo::authentication::AuthenticationStrategy;
    use crate::vojo::authentication::BasicAuth;
    use crate::vojo::rate_limit::*;
    use crate::vojo::route::BaseRoute;
    use crate::vojo::route::HeaderBasedRoute;
    use crate::vojo::route::HeaderRoute;
    use crate::vojo::route::PollBaseRoute;
    use crate::vojo::route::PollRoute;
    use crate::vojo::route::RandomBaseRoute;
    use crate::vojo::route::RandomRoute;

    use crate::vojo::health_check::{BaseHealthCheckParam, HttpHealthCheckParam};
    use crate::vojo::route::RegexMatch;
    use crate::vojo::route::WeightBasedRoute;
    use crate::vojo::route::WeightRoute;
    use dashmap::DashMap;
    use std::sync::atomic::AtomicIsize;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::RwLock;
    use std::time::SystemTime;
    fn create_new_route_with_host_name(host_name: Option<String>) -> Route {
        return Route {
            host_name: host_name,
            route_id: new_uuid(),
            route_cluster: Box::new(WeightBasedRoute {
                indexs: Default::default(),
                routes: vec![WeightRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                        health_check_status: Arc::new(RwLock::new(None)),
                    },
                    weight: 100,
                }],
            }),
            health_check: None,
            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("/"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
    }
    #[test]
    fn test_host_name_is_none_ok1() {
        let route = create_new_route_with_host_name(None);
        let mut headermap = HeaderMap::new();
        headermap.insert("x-client", "Basic bHNrOjEyMzQ=".parse().unwrap());
        let allow_result = route.is_matched("/test", Some(headermap));
        assert_eq!(allow_result.is_ok(), true);
        assert_eq!(allow_result.unwrap(), true);
    }
    #[test]
    fn test_host_name_is_some_ok2() {
        let route = create_new_route_with_host_name(Some(String::from("www.test.com")));
        let mut headermap = HeaderMap::new();
        headermap.insert("x-client", "Basic bHNrOjEyMzQ=".parse().unwrap());
        let allow_result = route.is_matched("/test", Some(headermap));
        assert_eq!(allow_result.is_ok(), true);
        assert_eq!(allow_result.unwrap(), false);
    }
    #[test]
    fn test_host_name_is_some_ok3() {
        let route = create_new_route_with_host_name(Some(String::from("www.test.com")));
        let mut headermap = HeaderMap::new();
        headermap.insert("Host", "Basic bHNrOjEyMzQ=".parse().unwrap());
        let allow_result = route.is_matched("/test", Some(headermap));
        assert_eq!(allow_result.is_ok(), true);
        assert_eq!(allow_result.unwrap(), false);
    }
    #[test]
    fn test_host_name_is_some_ok4() {
        let route = create_new_route_with_host_name(Some(String::from("www.test.com")));
        let mut headermap = HeaderMap::new();
        headermap.insert("Host", "www.test.com".parse().unwrap());
        let allow_result = route.is_matched("/test", Some(headermap));
        assert_eq!(allow_result.is_ok(), true);
        assert_eq!(allow_result.unwrap(), true);
    }
    #[test]
    fn test_serde_output_health_check() {
        let route = Route {
            host_name: None,
            route_id: new_uuid(),
            route_cluster: Box::new(WeightBasedRoute {
                indexs: Default::default(),
                routes: vec![WeightRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                        health_check_status: Arc::new(RwLock::new(None)),
                    },
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
            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
            api_service_id: new_uuid(),
            listen_port: 4486,
            service_config: ServiceConfig {
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
        let route = Route {
            host_name: None,
            route_id: new_uuid(),
            route_cluster: Box::new(WeightBasedRoute {
                indexs: Default::default(),
                routes: vec![WeightRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                        health_check_status: Arc::new(RwLock::new(None)),
                    },
                    weight: 100,
                }],
            }),
            health_check: None,
            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
            api_service_id: new_uuid(),
            listen_port: 4486,
            service_config: ServiceConfig {
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
        let route = Route {
            host_name: None,
            route_id: new_uuid(),
            route_cluster: Box::new(HeaderBasedRoute {
                routes: vec![HeaderRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                        health_check_status: Arc::new(RwLock::new(None)),
                    },
                    header_key: String::from("user-agent"),
                    header_value_mapping_type: crate::vojo::route::HeaderValueMappingType::REGEX(
                        RegexMatch {
                            value: String::from("^100$"),
                        },
                    ),
                }],
            }),
            health_check: None,
            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
            api_service_id: new_uuid(),
            listen_port: 4486,
            service_config: ServiceConfig {
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
        let route = Route {
            host_name: None,
            route_id: new_uuid(),
            route_cluster: Box::new(RandomRoute {
                routes: vec![
                    RandomBaseRoute {
                        base_route: BaseRoute {
                            endpoint: String::from("/"),
                            try_file: None,
                            health_check_status: Arc::new(RwLock::new(None)),
                        },
                    },
                    RandomBaseRoute {
                        base_route: BaseRoute {
                            endpoint: String::from("/"),
                            try_file: None,
                            health_check_status: Arc::new(RwLock::new(None)),
                        },
                    },
                ],
            }),
            allow_deny_list: None,
            authentication: None,
            health_check: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
            api_service_id: new_uuid(),
            listen_port: 4486,
            service_config: ServiceConfig {
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
        let route = Route {
            host_name: None,
            route_id: new_uuid(),
            route_cluster: Box::new(PollRoute {
                routes: vec![PollBaseRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                        health_check_status: Arc::new(RwLock::new(None)),
                    },
                }],
                lock: Default::default(),
                current_index: Default::default(),
            }),
            health_check: None,
            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
            api_service_id: new_uuid(),

            listen_port: 4486,
            service_config: ServiceConfig {
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
        let route = Route {
            host_name: None,
            route_id: new_uuid(),
            route_cluster: Box::new(PollRoute {
                routes: vec![PollBaseRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                        health_check_status: Arc::new(RwLock::new(None)),
                    },
                }],
                lock: Default::default(),
                current_index: Default::default(),
            }),
            health_check: None,
            allow_deny_list: None,
            authentication: Some(basic_auth),
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
            listen_port: 4486,
            api_service_id: new_uuid(),
            service_config: ServiceConfig {
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
        let route = Route {
            host_name: None,
            route_id: new_uuid(),
            route_cluster: Box::new(PollRoute {
                routes: vec![PollBaseRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                        health_check_status: Arc::new(RwLock::new(None)),
                    },
                }],
                lock: Default::default(),
                current_index: Default::default(),
            }),
            health_check: None,
            allow_deny_list: None,
            ratelimit: None,
            authentication: Some(api_key_auth),
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
            api_service_id: new_uuid(),
            listen_port: 4486,
            service_config: ServiceConfig {
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
        let route = Route {
            host_name: None,
            route_id: new_uuid(),
            route_cluster: Box::new(PollRoute {
                routes: vec![PollBaseRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                        health_check_status: Arc::new(RwLock::new(None)),
                    },
                }],
                lock: Default::default(),
                current_index: Default::default(),
            }),
            health_check: None,
            allow_deny_list: None,
            authentication: None,
            ratelimit: Some(ratelimit),
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
            api_service_id: new_uuid(),
            listen_port: 4486,
            service_config: ServiceConfig {
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
        let route = Route {
            host_name: None,
            route_id: new_uuid(),
            route_cluster: Box::new(PollRoute {
                routes: vec![PollBaseRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                        health_check_status: Arc::new(RwLock::new(None)),
                    },
                }],
                lock: Default::default(),
                current_index: Default::default(),
            }),
            health_check: None,
            allow_deny_list: None,
            authentication: None,
            ratelimit: Some(ratelimit),
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
            api_service_id: new_uuid(),
            listen_port: 4486,
            service_config: ServiceConfig {
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
            limit_type: crate::vojo::allow_deny_ip::AllowType::ALLOW,
            value: Some(String::from("sss")),
        };
        let route = Route {
            host_name: None,
            route_id: new_uuid(),
            route_cluster: Box::new(PollRoute {
                routes: vec![PollBaseRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                        health_check_status: Arc::new(RwLock::new(None)),
                    },
                }],
                lock: Default::default(),
                current_index: Default::default(),
            }),
            health_check: None,
            allow_deny_list: Some(vec![allow_object]),
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
            api_service_id: new_uuid(),
            listen_port: 4486,
            service_config: ServiceConfig {
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
}
