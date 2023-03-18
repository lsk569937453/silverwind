use crate::vojo::allow_deny_ip::AllowDenyObject;
use crate::vojo::route::LoadbalancerStrategy;
use serde::{Deserialize, Serialize};

use super::allow_deny_ip::AllowResult;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Matcher {
    pub prefix: String,
    pub prefix_rewrite: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub matcher: Option<Matcher>,
    pub allow_deny_list: Option<Vec<AllowDenyObject>>,
    pub route_cluster: Box<dyn LoadbalancerStrategy>,
}

impl Route {
    pub fn is_allowed(&self, ip: String) -> Result<bool, anyhow::Error> {
        if self.allow_deny_list == None || self.allow_deny_list.clone().unwrap().len() == 0 {
            return Ok(true);
        }
        let allow_deny_list = self.allow_deny_list.clone().unwrap();
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
    use crate::vojo::route::BaseRoute;
    use crate::vojo::route::RandomRoute;
    #[test]
    fn test_output_serde() {
        let route = Route {
            route_cluster: Box::new(RandomRoute {
                routes: vec![BaseRoute {
                    weight: 100,
                    endpoint: String::from("/"),
                    try_file: None,
                }],
            }),
            allow_deny_list: None,
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
