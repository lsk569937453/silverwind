use base64::{engine::general_purpose, Engine as _};
use core::fmt::Debug;
use dyn_clone::DynClone;
use http::HeaderMap;
use http::HeaderValue;
use log::Level;
use rand::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashSet;

use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
#[typetag::serde(tag = "type")]
pub trait AuthenticationStrategy: Sync + Send + DynClone {
    fn check_authentication(
        &mut self,
        headers: HeaderMap<HeaderValue>,
    ) -> Result<bool, anyhow::Error>;

    fn get_debug(&self) -> String {
        String::from("debug")
    }
    fn as_any(&self) -> &dyn Any;
}
dyn_clone::clone_trait_object!(AuthenticationStrategy);

impl Debug for dyn AuthenticationStrategy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let routes = self.get_debug().clone();
        write!(f, "{{{}}}", routes)
    }
}
//Basic bHNrOjEyMzQ=
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BasicAuth {
    pub credentials: String,
}
#[typetag::serde]
impl AuthenticationStrategy for BasicAuth {
    fn check_authentication(
        &mut self,
        headers: HeaderMap<HeaderValue>,
    ) -> Result<bool, anyhow::Error> {
        if headers.len() == 0 || !headers.contains_key("Authorization") {
            return Ok(false);
        }
        let value = headers
            .get("Authorization")
            .unwrap()
            .to_str()
            .map_err(|err| anyhow!(err.to_string()))?;
        let split_list: Vec<_> = value.split(" ").collect();
        if split_list.len() != 2 || split_list[0] != "Basic" {
            return Ok(false);
        }
        let encoded: String = general_purpose::STANDARD_NO_PAD.encode(self.credentials.clone());
        if split_list[1] != encoded {
            return Ok(false);
        }

        Ok(true)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ApiKeyAuth {
    pub key: String,
    pub value: String,
}

#[typetag::serde]
impl AuthenticationStrategy for ApiKeyAuth {
    fn check_authentication(
        &mut self,
        headers: HeaderMap<HeaderValue>,
    ) -> Result<bool, anyhow::Error> {
        if headers.len() == 0 || !headers.contains_key(self.key.clone()) {
            return Ok(false);
        }
        let header_value = headers
            .get(self.key.clone())
            .unwrap()
            .to_str()
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(header_value == self.value)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vojo::app_config::ApiService;
    use crate::vojo::route::BaseRoute;
    use crate::vojo::route::HeaderBasedRoute;
    use crate::vojo::route::HeaderRoute;
    use crate::vojo::route::PollRoute;
    use crate::vojo::route::RandomRoute;
    use crate::vojo::route::RegexMatch;
    use crate::vojo::route::WeightBasedRoute;
    use crate::vojo::route::WeightRoute;
    #[test]
    fn test_basic_auth_error1() {
        let mut basic_auth: Box<dyn AuthenticationStrategy> = Box::new(BasicAuth {
            credentials: String::from("lsk:password"),
        });
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("x-client", "Basic bHNrOjEyMzQ=".parse().unwrap());
        let res1 = basic_auth.check_authentication(headermap1);
        assert_eq!(res1.unwrap(), false);
    }
    #[test]
    fn test_basic_auth_error2() {
        let mut basic_auth: Box<dyn AuthenticationStrategy> = Box::new(BasicAuth {
            credentials: String::from("lsk:password"),
        });
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("Authorization", "BasicbHNrOjEyMzQ=".parse().unwrap());
        let res1 = basic_auth.check_authentication(headermap1);
        assert_eq!(res1.unwrap(), false);
    }
    #[test]
    fn test_basic_auth_error3() {
        let mut basic_auth: Box<dyn AuthenticationStrategy> = Box::new(BasicAuth {
            credentials: String::from("lsk:password"),
        });
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("Authorization", "Basic test".parse().unwrap());
        let res1 = basic_auth.check_authentication(headermap1);
        assert_eq!(res1.unwrap(), false);
    }
    #[test]
    fn test_basic_auth_ok() {
        let mut basic_auth: Box<dyn AuthenticationStrategy> = Box::new(BasicAuth {
            credentials: String::from("lsk:password"),
        });
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("Authorization", "Basic bHNrOnBhc3N3b3Jk".parse().unwrap());
        let res1 = basic_auth.check_authentication(headermap1);
        assert_eq!(res1.unwrap(), true);
    }
    #[test]
    fn test_api_key_auth_error() {
        let mut basic_auth: Box<dyn AuthenticationStrategy> = Box::new(ApiKeyAuth {
            key: String::from("sss"),
            value: String::from("test2"),
        });
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("Authorization", "Basic bHNrOnBhc3N3b3Jk".parse().unwrap());
        let res1 = basic_auth.check_authentication(headermap1);
        assert_eq!(res1.unwrap(), false);
    }

    #[test]
    fn test_api_key_auth_ok() {
        let mut basic_auth: Box<dyn AuthenticationStrategy> = Box::new(ApiKeyAuth {
            key: String::from("api_key"),
            value: String::from("test2"),
        });
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("api_key", "test2".parse().unwrap());
        let res1 = basic_auth.check_authentication(headermap1);
        assert_eq!(res1.unwrap(), true);
    }
    #[test]
    fn test_basic_auth_as_any() {
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
                    "authentication": {
                      "type": "BasicAuth",
                      "credentials": "lsk:123456"
                    },
                    "route_cluster": {
                      "type": "PollRoute",
                      "routes": [
                        {
                          "endpoint": "/",
                          "try_file": null
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
        let route = first_api_service.authentication.unwrap();
        let basic_auth: &BasicAuth = match route.as_any().downcast_ref::<BasicAuth>() {
            Some(b) => b,
            None => panic!("error!"),
        };

        assert_eq!(basic_auth.credentials, "lsk:123456");
    }

    #[test]
    fn test_api_key_auth_as_any() {
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
                    "authentication": {
                      "type": "ApiKeyAuth",
                      "key": "api_key",
                      "value": "test"
                    },
                    "route_cluster": {
                      "type": "PollRoute",
                      "routes": [
                        {
                          "endpoint": "/",
                          "try_file": null
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
        let route = first_api_service.authentication.unwrap();
        let basic_auth: &ApiKeyAuth = match route.as_any().downcast_ref::<ApiKeyAuth>() {
            Some(b) => b,
            None => panic!("error!"),
        };

        assert_eq!(basic_auth.key, "api_key");
        assert_eq!(basic_auth.value, "test");
    }
}
