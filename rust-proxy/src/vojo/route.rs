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
pub trait LoadbalancerStrategy: Sync + Send + DynClone {
    fn get_route(&mut self, headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, anyhow::Error>;

    fn get_debug(&self) -> String {
        String::from("debug")
    }
    fn as_any(&self) -> &dyn Any;
}
dyn_clone::clone_trait_object!(LoadbalancerStrategy);

impl Debug for dyn LoadbalancerStrategy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let routes = self.get_debug().clone();
        write!(f, "{{{}}}", routes)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BaseRoute {
    pub endpoint: String,
    pub try_file: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WeightRoute {
    pub base_route: BaseRoute,
    #[serde(default = "default_weight")]
    pub weight: i32,
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
    REGEX(RegexMatch),
    TEXT(TextMatch),
    SPLIT(SplitSegment),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeaderRoute {
    pub base_route: BaseRoute,
    pub header_key: String,
    pub header_value_mapping_type: HeaderValueMappingType,
}

fn default_weight() -> i32 {
    100
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct HeaderBasedRoute {
    pub routes: Vec<HeaderRoute>,
}

#[typetag::serde]
impl LoadbalancerStrategy for HeaderBasedRoute {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn get_route(&mut self, headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, anyhow::Error> {
        for item in self.routes.iter() {
            let headers_contais_key = headers.contains_key(item.header_key.clone());
            if !headers_contais_key {
                continue;
            }
            let header_value = headers.get(item.header_key.clone()).unwrap();
            let header_value_str = header_value.to_str().unwrap();
            match item.clone().header_value_mapping_type {
                HeaderValueMappingType::REGEX(regex_str) => {
                    let re = Regex::new(&regex_str.value).unwrap();
                    let capture_option = re.captures(header_value_str);
                    if capture_option.is_none() {
                        continue;
                    } else {
                        return Ok(item.clone().base_route);
                    }
                }
                HeaderValueMappingType::TEXT(text_str) => {
                    if text_str.value == header_value_str {
                        return Ok(item.clone().base_route);
                    } else {
                        continue;
                    }
                }
                HeaderValueMappingType::SPLIT(split_segment) => {
                    let split_set: HashSet<_> =
                        header_value_str.split(&split_segment.split_by).collect();
                    if split_set.len() == 0 {
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

        let first = self.routes.first().unwrap().base_route.clone();
        Ok(first)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RandomBaseRoute {
    pub base_route: BaseRoute,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RandomRoute {
    pub routes: Vec<RandomBaseRoute>,
}
#[typetag::serde]
impl LoadbalancerStrategy for RandomRoute {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn get_route(&mut self, _headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, anyhow::Error> {
        let mut rng = thread_rng();
        let index = rng.gen_range(0..self.routes.len());
        let dst = self.routes[index].clone();
        Ok(dst.base_route)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PollBaseRoute {
    pub base_route: BaseRoute,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PollRoute {
    #[serde(skip_serializing, skip_deserializing)]
    pub current_index: Arc<AtomicUsize>,
    pub routes: Vec<PollBaseRoute>,
    #[serde(skip_serializing, skip_deserializing)]
    pub lock: Arc<Mutex<i32>>,
}
#[typetag::serde]
impl LoadbalancerStrategy for PollRoute {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn get_route(&mut self, _headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, anyhow::Error> {
        let older = self.current_index.fetch_add(1, Ordering::SeqCst);
        let len = self.routes.len();
        let current_index = older % len;
        let dst = self.routes[current_index].clone();
        if log_enabled!(Level::Debug) {
            debug!("PollRoute current index:{}", current_index as i32);
        }
        Ok(dst.base_route)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WeightBasedRoute {
    #[serde(skip_serializing, skip_deserializing)]
    pub indexs: Arc<RwLock<Vec<AtomicIsize>>>,
    pub routes: Vec<WeightRoute>,
}

impl WeightRoute {}
#[typetag::serde]
impl LoadbalancerStrategy for WeightBasedRoute {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn get_route(&mut self, _headers: HeaderMap<HeaderValue>) -> Result<BaseRoute, anyhow::Error> {
        let indexs = self.indexs.read().unwrap();
        for (pos, e) in indexs.iter().enumerate() {
            let old_value = e.fetch_sub(1, Ordering::SeqCst);
            if old_value > 0 {
                let res = &self.routes[pos].clone();
                if log_enabled!(Level::Debug) {
                    debug!("WeightRoute current index:{}", pos as i32);
                }
                return Ok(res.base_route.clone());
            }
        }
        drop(indexs);

        let mut new_lock = self.indexs.write().unwrap();
        let check_alive = new_lock.iter().any(|f| {
            let tt = f.load(Ordering::SeqCst);
            tt.is_positive()
        });
        if !check_alive {
            let s = self
                .routes
                .iter()
                .map(|s| AtomicIsize::from(s.weight as isize))
                .collect::<Vec<AtomicIsize>>();
            *new_lock = s;
        }
        drop(new_lock);

        let indexs = self.indexs.read().unwrap();
        for (pos, e) in indexs.iter().enumerate() {
            let old_value = e.fetch_sub(1, Ordering::SeqCst);
            if old_value > 0 {
                let res = &self.routes[pos].clone();
                if log_enabled!(Level::Debug) {
                    debug!("WeightRoute current index:{}", pos as i32);
                }
                return Ok(res.base_route.clone());
            }
        }
        Err(anyhow!("WeightRoute get route error"))
    }
}
#[cfg(test)]
mod tests {
    use std::vec;

    use super::*;
    use crate::vojo::app_config::ApiService;
    fn get_routes() -> Vec<BaseRoute> {
        vec![
            BaseRoute {
                endpoint: String::from("http://localhost:4444"),
                try_file: None,
            },
            BaseRoute {
                endpoint: String::from("http://localhost:5555"),
                try_file: None,
            },
            BaseRoute {
                endpoint: String::from("http://localhost:5555"),
                try_file: None,
            },
        ]
    }
    fn get_random_routes() -> Vec<RandomBaseRoute> {
        vec![
            RandomBaseRoute {
                base_route:{BaseRoute{
                endpoint: String::from("http://localhost:4444"),
                try_file: None,}}
            },
            RandomBaseRoute {
                base_route:{BaseRoute{
                endpoint: String::from("http://localhost:5555"),
                try_file: None,}}
            },
            RandomBaseRoute {
                base_route:{BaseRoute{
                endpoint: String::from("http://localhost:5555"),
                try_file: None,}}
            },
        ]
    }
    fn get_poll_routes() -> Vec<PollBaseRoute> {
        vec![
            PollBaseRoute {
                base_route:{BaseRoute{
                endpoint: String::from("http://localhost:4444"),
                try_file: None,}}
            },
            PollBaseRoute {
                base_route:{BaseRoute{
                endpoint: String::from("http://localhost:5555"),
                try_file: None,}}
            },PollBaseRoute {
                base_route:{BaseRoute{
                endpoint: String::from("http://localhost:5555"),
                try_file: None,}}
            },
        ]
    }
    fn get_weight_routes() -> Vec<WeightRoute> {
        vec![
            WeightRoute {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:4444"),
                    try_file: None,
                },
                weight: 100,
            },
            WeightRoute {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:5555"),
                    try_file: None,
                },
                weight: 100,
            },
            WeightRoute {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:6666"),
                    try_file: None,
                },
                weight: 100,
            },
        ]
    }
    fn get_header_based_routes() -> Vec<HeaderRoute> {
        vec![
            HeaderRoute {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:4444"),
                    try_file: None,
                },
                header_key: String::from("x-client"),
                header_value_mapping_type: HeaderValueMappingType::REGEX(RegexMatch {
                    value: String::from("^100*"),
                }),
            },
            HeaderRoute {
                base_route: BaseRoute {
                    endpoint: String::from("http://localhost:5555"),
                    try_file: None,
                },
                header_key: String::from("x-client"),
                header_value_mapping_type: HeaderValueMappingType::SPLIT(SplitSegment {
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
                },
                header_key: String::from("x-client"),
                header_value_mapping_type: HeaderValueMappingType::SPLIT(SplitSegment {
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
                },
                header_key: String::from("x-client"),
                header_value_mapping_type: HeaderValueMappingType::TEXT(TextMatch {
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
    #[test]
    fn test_poll_route_successfully() {
        let routes = get_poll_routes();
        let mut poll_rate = PollRoute {
            current_index: Default::default(),
            routes: routes.clone(),
            lock: Default::default(),
        };
        for i in 0..100 {
            let current_route = poll_rate.get_route(HeaderMap::new()).unwrap();
            assert_eq!(current_route, routes[i % routes.len()].base_route);
        }
    }
    #[test]
    fn test_random_route_successfully() {
        let routes = get_random_routes();
        let mut random_rate = RandomRoute {
            routes: routes.clone(),
        };
        for _ in 0..100 {
            random_rate.get_route(HeaderMap::new()).unwrap();
        }
    }
    #[test]
    fn test_weight_route_successfully() {
        let routes = get_weight_routes();
        let mut weight_route = WeightBasedRoute {
            indexs: Default::default(),
            routes: routes.clone(),
        };
        for _ in 0..100 {
            let current_route = weight_route.get_route(HeaderMap::new()).unwrap();
            assert_eq!(current_route, routes[0].base_route);
        }
        for _ in 0..100 {
            let current_route = weight_route.get_route(HeaderMap::new()).unwrap();
            assert_eq!(current_route, routes[1].base_route);
        }
        for _ in 0..100 {
            let current_route = weight_route.get_route(HeaderMap::new()).unwrap();
            assert_eq!(current_route, routes[2].base_route);
        }
        for _ in 0..100 {
            let current_route = weight_route.get_route(HeaderMap::new()).unwrap();
            assert_eq!(current_route, routes[0].base_route);
        }
    }
    #[test]
    fn test_debug_trait() {
        let weight_route: Box<dyn LoadbalancerStrategy> = Box::new(WeightBasedRoute {
            indexs: Default::default(),
            routes: Default::default(),
        });
        assert_eq!(format!("{:?}", weight_route), "{debug}");
    }
    #[test]
    fn test_serde_default_weight() {
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
                    "route_cluster": {
                      "type": "WeightBasedRoute",
                      "routes": [
                        {
                          "base_route": {
                            "endpoint": "/",
                            "try_file": null
                          }
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
        let route = first_api_service.route_cluster;
        let weight_based_route: &WeightBasedRoute =
            match route.as_any().downcast_ref::<WeightBasedRoute>() {
                Some(b) => b,
                None => panic!("&a isn't a B!"),
            };
        assert_eq!(weight_based_route.routes.first().unwrap().weight, 100)
    }

    #[test]
    fn test_header_based_route_as_any() {
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
                    "route_cluster": {
                      "type": "HeaderBasedRoute",
                      "routes": [
                        {
                          "base_route": {
                            "endpoint": "/",
                            "try_file": null
                          },
                          "header_key": "user-agent",
                          "header_value_mapping_type": {
                            "type": "REGEX",
                            "value": "^100$"
                          }
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
        let route = first_api_service.route_cluster;
        let header_based_route: &HeaderBasedRoute =
            match route.as_any().downcast_ref::<HeaderBasedRoute>() {
                Some(b) => b,
                None => panic!("&a isn't a B!"),
            };
        let regex_match = RegexMatch {
            value: String::from("^100$"),
        };
        assert_eq!(
            header_based_route
                .routes
                .first()
                .unwrap()
                .header_value_mapping_type,
            HeaderValueMappingType::REGEX(regex_match)
        )
    }
    #[test]
    fn test_random_route_as_any() {
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
                    "route_cluster": {
                      "type": "RandomRoute",
                      "routes": [
                        {
                            "base_route": {
                                "endpoint": "/",
                                "try_file": null
                            }
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
        let route = first_api_service.route_cluster;
        let header_based_route: &RandomRoute = match route.as_any().downcast_ref::<RandomRoute>() {
            Some(b) => b,
            None => panic!("&a isn't a B!"),
        };

        assert_eq!(header_based_route.routes.first().unwrap().base_route.endpoint, "/");
    }
    #[test]
    fn test_poll_route_as_any() {
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
                    "route_cluster": {
                      "type": "PollRoute",
                      "routes": [
                        {
                            "base_route": {
                                "endpoint": "/",
                                "try_file": null
                            }
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
        let route = first_api_service.route_cluster;
        let header_based_route: &PollRoute = match route.as_any().downcast_ref::<PollRoute>() {
            Some(b) => b,
            None => panic!("&a isn't a B!"),
        };

        assert_eq!(header_based_route.routes.first().unwrap().base_route.endpoint, "/");
    }
    #[test]
    fn test_header_based_route_successfully() {
        let routes = get_header_based_routes();
        let header_route = HeaderBasedRoute { routes: routes };
        let mut header_route: Box<dyn LoadbalancerStrategy> = Box::new(header_route);
        let mut headermap1 = HeaderMap::new();
        headermap1.insert("x-client", "100zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap());
        let result1 = header_route.get_route(headermap1.clone());
        assert_eq!(result1.is_ok(), true);
        assert_eq!(result1.unwrap().endpoint, "http://localhost:4444");

        let mut headermap2 = HeaderMap::new();
        headermap2.insert("x-client", "a=1;b=2;c:3;d=4;f5=6667".parse().unwrap());
        let result2 = header_route.get_route(headermap2.clone());
        assert_eq!(result2.is_ok(), true);
        assert_eq!(result2.unwrap().endpoint, "http://localhost:5555");

        let mut headermap3 = HeaderMap::new();
        headermap3.insert("x-client", "a:12,b:9,c=7,d=4;f5=6667".parse().unwrap());
        let result3 = header_route.get_route(headermap3.clone());
        assert_eq!(result3.is_ok(), true);
        assert_eq!(result3.unwrap().endpoint, "http://localhost:7777");

        let mut headermap4 = HeaderMap::new();
        headermap4.insert("x-client", "google chrome".parse().unwrap());
        let result4 = header_route.get_route(headermap4.clone());
        assert_eq!(result4.is_ok(), true);
        assert_eq!(result4.unwrap().endpoint, "http://localhost:8888");
    }
}
