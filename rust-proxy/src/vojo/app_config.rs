use super::allow_deny_ip::AllowResult;

use crate::vojo::allow_deny_ip::AllowDenyObject;
use crate::vojo::anomaly_detection::AnomalyDetectionType;

use crate::vojo::app_error::AppError;
use crate::vojo::authentication::AuthenticationStrategy;
use crate::vojo::health_check::HealthCheckType;
use crate::vojo::rate_limit::RatelimitStrategy;
use crate::vojo::route::LoadbalancerStrategy;
use http::HeaderMap;
use http::HeaderValue;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    #[serde(skip)]
    pub route_id: String,
    pub host_name: Option<String>,
    pub matcher: Option<Matcher>,
    pub allow_deny_list: Option<Vec<AllowDenyObject>>,
    pub authentication: Option<Box<dyn AuthenticationStrategy>>,
    pub anomaly_detection: Option<AnomalyDetectionType>,
    #[serde(skip)]
    pub liveness_status: LivenessStatus,
    pub rewrite_headers: Option<HashMap<String, String>>,
    pub liveness_config: Option<LivenessConfig>,
    pub health_check: Option<HealthCheckType>,
    pub ratelimit: Option<Box<dyn RatelimitStrategy>>,
    pub route_cluster: LoadbalancerStrategy,
}

impl Route {
    pub fn is_matched(
        &self,
        path: String,
        headers_option: Option<HeaderMap<HeaderValue>>,
    ) -> Result<Option<String>, AppError> {
        let matcher = self
            .clone()
            .matcher
            .ok_or("The matcher counld not be none for http")
            .map_err(|err| AppError(err.to_string()))?;

        let match_res = path.strip_prefix(matcher.prefix.as_str());
        if match_res.is_none() {
            return Ok(None);
        }
        let final_path = format!("{}{}", matcher.prefix_rewrite, match_res.unwrap());
        // info!("final_path:{}", final_path);
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
            let host_name_regex =
                Regex::new(real_host_name.as_str()).map_err(|e| AppError(e.to_string()))?;
            return host_name_regex
                .captures(host_result.unwrap())
                .map_or(Ok(None), |_| Ok(Some(final_path)));
        }
        Ok(Some(final_path))
    }
    pub async fn is_allowed(
        &self,
        ip: String,
        headers_option: Option<HeaderMap<HeaderValue>>,
    ) -> Result<bool, AppError> {
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
) -> Result<bool, AppError> {
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
                return Err(AppError(err.to_string()));
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
    Http2,
    Http2Tls,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub server_type: ServiceType,
    pub cert_str: Option<String>,
    pub key_str: Option<String>,
    pub routes: Vec<Route>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiService {
    pub listen_port: i32,
    #[serde(skip)]
    pub api_service_id: String,
    pub service_config: ServiceConfig,
    #[serde(skip, default = "default_sender")]
    pub sender: mpsc::Sender<()>,
}
fn default_sender() -> mpsc::Sender<()> {
    let (sender, receiver) = mpsc::channel(1);
    sender
}

impl Default for ApiService {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel(1);
        Self {
            listen_port: 0, // Provide default values for each field
            api_service_id: String::default(),
            service_config: ServiceConfig::default(), // Initialize ServiceConfig with its default value
            sender,                                   // Use the default value for Sender<()>
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StaticConfig {
    pub access_log: Option<String>,
    pub database_url: Option<String>,
    pub admin_port: String,
    pub config_file_path: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    pub static_config: StaticConfig,
    pub api_service_config: HashMap<String, ApiService>,
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_plane::rest_api::get_router;
    use crate::utils::uuid::get_uuid;

    use crate::vojo::health_check::{BaseHealthCheckParam, HttpHealthCheckParam};
    use crate::vojo::rate_limit::*;
    use crate::vojo::route::AnomalyDetectionStatus;
    use crate::vojo::route::BaseRoute;

    use crate::vojo::route::WeightRoute;
    use crate::vojo::route::WeightRouteNestedItem;
    use std::sync::atomic::AtomicIsize;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::SystemTime;
    use tokio::sync::RwLock;
    fn create_new_route_with_host_name(host_name: Option<String>) -> Route {
        Route {
            host_name,
            route_id: get_uuid(),
            route_cluster: LoadbalancerStrategy::WeightRoute(WeightRoute {
                index: 0,
                offset: 0,
                routes: vec![WeightRouteNestedItem {
                    base_route: BaseRoute {
                        base_route_id: "a".to_string(),
                        endpoint: String::from("/"),
                        try_file: None,
                        is_alive: None,
                        anomaly_detection_status: AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        },
                    },
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
            rewrite_headers: None,
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
        let allow_result = route.is_matched(String::from("/test"), Some(headermap));
        assert!(allow_result.is_ok());
        assert!(allow_result.unwrap().is_some());
    }

    #[test]
    fn test_host_name_is_some_ok2() {
        let route = create_new_route_with_host_name(Some(String::from("www.test.com")));
        let mut headermap = HeaderMap::new();
        headermap.insert("x-client", "Basic bHNrOjEyMzQ=".parse().unwrap());
        let allow_result = route.is_matched(String::from("/test"), Some(headermap));
        assert!(allow_result.is_ok());
        assert!(allow_result.unwrap().is_none());
    }
    #[test]
    fn test_host_name_is_some_ok3() {
        let route = create_new_route_with_host_name(Some(String::from("www.test.com")));
        let mut headermap = HeaderMap::new();
        headermap.insert("Host", "Basic bHNrOjEyMzQ=".parse().unwrap());
        let allow_result = route.is_matched(String::from("/test"), Some(headermap));
        assert!(allow_result.is_ok());
        assert!(allow_result.unwrap().is_none());
    }
    #[test]
    fn test_host_name_is_some_ok4() {
        let route = create_new_route_with_host_name(Some(String::from("www.test.com")));
        let mut headermap = HeaderMap::new();
        headermap.insert("Host", "www.test.com".parse().unwrap());
        let allow_result = route.is_matched(String::from("/test"), Some(headermap));
        assert!(allow_result.is_ok());
        assert!(allow_result.unwrap().is_some());
    }
    #[test]
    fn test_serde_output_health_check() {
        let route = create_new_route_with_host_name(Some("http://httpbin.org".to_string()));
        let yaml = serde_yaml::to_string(&route).unwrap();
        println!("{}", yaml);
    }

    #[test]
    fn test_regex() {
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
