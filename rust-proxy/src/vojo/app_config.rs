use super::allow_deny_ip::AllowResult;
use crate::vojo::allow_deny_ip::AllowDenyObject;
use crate::vojo::authentication::AuthenticationStrategy;
use crate::vojo::rate_limit::RatelimitStrategy;
use crate::vojo::route::LoadbalancerStrategy;
use http::HeaderMap;
use http::HeaderValue;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Matcher {
    pub prefix: String,
    pub prefix_rewrite: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub matcher: Option<Matcher>,
    pub allow_deny_list: Option<Vec<AllowDenyObject>>,
    pub authentication: Option<Box<dyn AuthenticationStrategy>>,
    pub ratelimit: Option<Box<dyn RatelimitStrategy>>,
    pub route_cluster: Box<dyn LoadbalancerStrategy>,
}

impl Route {
    pub fn is_allowed(
        &self,
        ip: String,
        headers_option: Option<HeaderMap<HeaderValue>>,
    ) -> Result<bool, anyhow::Error> {
        let mut is_allowed = ip_is_allowed(self.allow_deny_list.clone(), ip.clone())?;
        if headers_option.is_some() && self.authentication.is_some() {
            is_allowed = self
                .authentication
                .clone()
                .unwrap()
                .check_authentication(headers_option.clone().unwrap())?;
        }
        if headers_option.is_some() && self.ratelimit.is_some() {
            let mut ratelimit = self.clone().ratelimit.unwrap();
            is_allowed = ratelimit.should_limit(headers_option.clone().unwrap(), ip)?;
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
    pub service_config: ServiceConfig,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StaticConifg {
    pub access_log: Option<String>,
    pub database_url: Option<String>,
    pub api_port: String,
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
    use crate::vojo::route::PollRoute;
    use crate::vojo::route::RandomRoute;
    use crate::vojo::route::RegexMatch;
    use crate::vojo::route::WeightBasedRoute;
    use crate::vojo::route::WeightRoute;
    use dashmap::DashMap;
    use std::sync::atomic::AtomicIsize;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::SystemTime;

    #[test]
    fn test_serde_output_weight_based_route() {
        let route = Route {
            route_cluster: Box::new(WeightBasedRoute {
                indexs: Default::default(),
                routes: vec![WeightRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                    },
                    weight: 100,
                }],
            }),
            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
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
            route_cluster: Box::new(HeaderBasedRoute {
                routes: vec![HeaderRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("/"),
                        try_file: None,
                    },
                    header_key: String::from("user-agent"),
                    header_value_mapping_type: crate::vojo::route::HeaderValueMappingType::REGEX(
                        RegexMatch {
                            value: String::from("^100$"),
                        },
                    ),
                }],
            }),
            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
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
            route_cluster: Box::new(RandomRoute {
                routes: vec![BaseRoute {
                    endpoint: String::from("/"),
                    try_file: None,
                }],
            }),
            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
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
            route_cluster: Box::new(PollRoute {
                routes: vec![BaseRoute {
                    endpoint: String::from("/"),
                    try_file: None,
                }],
                lock: Default::default(),
                current_index: Default::default(),
            }),
            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
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
            route_cluster: Box::new(PollRoute {
                routes: vec![BaseRoute {
                    endpoint: String::from("/"),
                    try_file: None,
                }],
                lock: Default::default(),
                current_index: Default::default(),
            }),
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
            route_cluster: Box::new(PollRoute {
                routes: vec![BaseRoute {
                    endpoint: String::from("/"),
                    try_file: None,
                }],
                lock: Default::default(),
                current_index: Default::default(),
            }),
            allow_deny_list: None,
            ratelimit: None,
            authentication: Some(api_key_auth),
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
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
            current_count: Arc::new(AtomicIsize::new(3)),
            lock: Arc::new(Mutex::new(0)),
            last_update_time: SystemTime::now(),
        };
        let ratelimit: Box<dyn RatelimitStrategy> = Box::new(token_bucket_ratelimit);
        let route = Route {
            route_cluster: Box::new(PollRoute {
                routes: vec![BaseRoute {
                    endpoint: String::from("/"),
                    try_file: None,
                }],
                lock: Default::default(),
                current_index: Default::default(),
            }),
            allow_deny_list: None,
            authentication: None,
            ratelimit: Some(ratelimit),
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
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
            count_map: DashMap::new(),
            lock: Arc::new(Mutex::new(0)),
        };
        let ratelimit: Box<dyn RatelimitStrategy> = Box::new(fixed_window_ratelimit);
        let route = Route {
            route_cluster: Box::new(PollRoute {
                routes: vec![BaseRoute {
                    endpoint: String::from("/"),
                    try_file: None,
                }],
                lock: Default::default(),
                current_index: Default::default(),
            }),
            allow_deny_list: None,
            authentication: None,
            ratelimit: Some(ratelimit),
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service = ApiService {
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
