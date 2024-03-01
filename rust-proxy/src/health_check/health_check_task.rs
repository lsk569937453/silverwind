use crate::configuration_service::app_config_service::GLOBAL_CONFIG_MAPPING;
use crate::constants::common_constants::TIMER_WAIT_SECONDS;
use crate::proxy::http1::http_client::HttpClients;
use crate::vojo::app_config::Route;
use crate::vojo::health_check::HealthCheckType;
use crate::vojo::health_check::HttpHealthCheckParam;
use bytes::Bytes;
use futures;
use futures::future::join_all;
use futures::FutureExt;
use http::Request;
use http::StatusCode;
use http_body_util::BodyExt;
use http_body_util::Full;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use url::Url;

use std::sync::Arc;
use tokio::task::JoinSet;
use tokio::time::sleep;

#[derive(Clone)]
pub struct HealthCheckClient {
    pub http_clients: HttpClients,
}
impl HealthCheckClient {
    pub fn new() -> Self {
        HealthCheckClient {
            http_clients: HttpClients::new(),
        }
    }
}
#[derive(Hash, Clone, Eq, PartialEq, Debug)]
pub struct TaskKey {
    pub route_id: String,
    pub health_check_type: HealthCheckType,
    pub endpoint_list: Vec<String>,
    pub min_liveness_count: i32,
}
impl TaskKey {
    pub fn new(
        route_id: String,
        health_check_type: HealthCheckType,
        endpoint_list: Vec<String>,
        min_liveness_count: i32,
    ) -> Self {
        TaskKey {
            route_id,
            health_check_type,
            endpoint_list,
            min_liveness_count,
        }
    }
}
async fn get_endpoint_list(mut route: Route) -> Vec<String> {
    let mut result = vec![];
    let base_route_list = route.route_cluster.get_all_route().await.unwrap_or(vec![]);
    for item in base_route_list {
        result.push(item.endpoint);
    }
    result
}
pub struct HealthCheck {
    pub task_id_map: HashMap<TaskKey, u64>,
    pub health_check_client: HealthCheckClient,
    pub current_id: Arc<AtomicU64>,
}
impl HealthCheck {
    pub fn new() -> Self {
        HealthCheck {
            task_id_map: HashMap::new(),
            health_check_client: HealthCheckClient::new(),
            current_id: Arc::new(AtomicU64::new(0)),
        }
    }
    pub async fn start_health_check_loop(&mut self) {
        loop {
            let async_result = std::panic::AssertUnwindSafe(self.do_health_check())
                .catch_unwind()
                .await;
            if async_result.is_err() {
                error!("start_health_check_loop catch panic successfully!");
            }
            sleep(std::time::Duration::from_secs(TIMER_WAIT_SECONDS)).await;
        }
    }

    async fn do_health_check(&mut self) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

async fn do_http_health_check(
    http_health_check_param: HttpHealthCheckParam,
    mut route: Route,
    timeout_number: i32,
    http_health_check_client: HealthCheckClient,
) -> Result<(), anyhow::Error> {
    let route_list = route.route_cluster.get_all_route().await?;
    let http_client = http_health_check_client.http_clients.clone();
    let mut set = JoinSet::new();
    for item in route_list {
        let http_client_shared = http_client.clone();
        let host_option = Url::parse(item.endpoint.as_str());
        if host_option.is_err() {
            error!(
                "Parse host error,the error is {}",
                host_option.unwrap_err().to_string()
            );
            continue;
        }

        let join_option = host_option
            .unwrap()
            .join(http_health_check_param.path.clone().as_str());
        if join_option.is_err() {
            error!(
                "Parse host error,the error is {}",
                join_option.unwrap_err().to_string()
            );
            continue;
        }

        let req = Request::builder()
            .uri(join_option.unwrap().to_string())
            .method("GET")
            .body(Full::new(Bytes::new()).boxed())
            .unwrap();
        let task_with_timeout = http_client_shared
            .clone()
            .request_http(req, timeout_number as u64);
        set.spawn(async {
            let res = task_with_timeout.await;
            (res, item)
        });
    }
    while let Some(response_result1) = set.join_next().await {
        if let Ok((response_result2, base_route)) = response_result1 {
            match response_result2 {
                Ok(Ok(t)) => {
                    if t.status() == StatusCode::OK {
                        base_route
                            .update_health_check_status_with_ok(route.liveness_status.clone())
                            .await;
                    }
                }
                _ => {
                    if let Some(current_liveness_config) = route.liveness_config.clone() {
                        let _update_result = base_route
                            .update_health_check_status_with_fail(
                                route.liveness_status.clone(),
                                current_liveness_config,
                            )
                            .await;
                    } else {
                        error!(
                            "Can not update the route-{} to fail,as the liveness_status is empty!",
                            base_route.endpoint.clone()
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
fn submit_task(
    task_id: u64,
    route: Route,
    health_check_clients: HealthCheckClient,
) -> Result<(), anyhow::Error> {
    if let Some(health_check) = route.health_check.clone() {
        let base_param = health_check.get_base_param();
        let timeout = base_param.timeout;
        let task = move || {
            let route_share = route.clone();
            let timeout_share = timeout;
            let health_check_client_shared = health_check_clients.clone();
            let health_check_type_shared = health_check.clone();
            async move {
                match health_check_type_shared {
                    HealthCheckType::HttpGet(http_health_check_param) => {
                        do_http_health_check(
                            http_health_check_param,
                            route_share,
                            timeout_share,
                            health_check_client_shared,
                        )
                        .await
                    }
                    HealthCheckType::Mysql(_) => Ok(()),
                    HealthCheckType::Redis(_) => Ok(()),
                }
            }
        };
        info!(
            "The timer task has been submit,the task param is interval:{}!",
            base_param.interval
        );
        return Ok(());
    }
    Err(anyhow!("Submit task error!"))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vojo::api_service_manager::ApiServiceManager;
    use crate::vojo::app_config::LivenessConfig;
    use crate::vojo::app_config::LivenessStatus;
    use crate::vojo::app_config::Matcher;
    use crate::vojo::app_config::ServiceConfig;
    use crate::vojo::health_check::BaseHealthCheckParam;
    use crate::vojo::route::AnomalyDetectionStatus;
    use crate::vojo::route::LoadbalancerStrategy;
    use crate::vojo::route::{BaseRoute, WeightBasedRoute, WeightRoute};
    use lazy_static::lazy_static;
    use std::sync::atomic::AtomicIsize;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::runtime::{Builder, Runtime};
    use tokio::sync::RwLock;

    use uuid::Uuid;
    lazy_static! {
        pub static ref TOKIO_RUNTIME: Runtime = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("my-custom-name")
            .thread_stack_size(3 * 1024 * 1024)
            .max_blocking_threads(1000)
            .enable_all()
            .build()
            .unwrap();
    }
    #[test]
    fn test_submit_task_error1() {
        let id = Uuid::new_v4();
        let route = Route {
            host_name: None,
            route_id: id.to_string(),
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
                    weight: 100,
                    index: Arc::new(AtomicIsize::new(0)),
                }])),
            }),
            liveness_status: Arc::new(RwLock::new(LivenessStatus {
                current_liveness_count: 0,
            })),
            anomaly_detection: None,
            health_check: None,
            liveness_config: None,
            allow_deny_list: None,
            rewrite_headers: None,

            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let health_check_param = HealthCheckClient::new();
        let res = submit_task(0, route, health_check_param);
        assert!(res.is_err());
    }
    #[tokio::test]
    async fn test_submit_task_ok1() {
        let id = Uuid::new_v4();
        let route = Route {
            host_name: None,
            route_id: id.to_string(),
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
                    weight: 100,
                    index: Arc::new(AtomicIsize::new(0)),
                }])),
            }),
            health_check: Some(HealthCheckType::HttpGet(HttpHealthCheckParam {
                base_health_check_param: BaseHealthCheckParam {
                    timeout: 10,
                    interval: 1,
                },
                path: String::from("value"),
            })),
            liveness_status: Arc::new(RwLock::new(LivenessStatus {
                current_liveness_count: 0,
            })),
            anomaly_detection: None,
            liveness_config: None,
            rewrite_headers: None,

            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let health_check_param = HealthCheckClient::new();
        let res = submit_task(0, route, health_check_param);
        assert!(res.is_ok());

        sleep(Duration::from_secs(2)).await;
        assert!(res.is_ok());
    }
    #[test]
    fn test_do_health_check_ok1() {
        let id = Uuid::new_v4();
        let (sender, _receiver) = tokio::sync::mpsc::channel(10);
        let route = Route {
            host_name: None,
            route_id: id.to_string(),
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
                    weight: 100,
                    index: Arc::new(AtomicIsize::new(0)),
                }])),
            }),
            health_check: Some(HealthCheckType::HttpGet(HttpHealthCheckParam {
                base_health_check_param: BaseHealthCheckParam {
                    timeout: 10,
                    interval: 10,
                },
                path: String::from("value"),
            })),
            anomaly_detection: None,
            liveness_config: None,
            rewrite_headers: None,

            liveness_status: Arc::new(RwLock::new(LivenessStatus {
                current_liveness_count: 0,
            })),
            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service_manager = ApiServiceManager {
            sender,
            service_config: ServiceConfig {
                key_str: None,
                server_type: crate::vojo::app_config::ServiceType::Https,
                cert_str: None,
                routes: vec![route],
            },
        };
        let uuid2 = Uuid::new_v4();
        let key = uuid2.to_string();

        TOKIO_RUNTIME.block_on(async move {
            GLOBAL_CONFIG_MAPPING.insert(key.clone(), api_service_manager);

            let mut health_check = HealthCheck::new();
            let res = health_check.do_health_check().await;
            assert!(res.is_ok());
        });
    }

    #[tokio::test]
    async fn test_do_health_check_ok2() {
        let id = Uuid::new_v4();
        let (sender, _receiver) = tokio::sync::mpsc::channel(10);
        let route = Route {
            host_name: None,
            route_id: id.to_string(),
            route_cluster: LoadbalancerStrategy::WeightBased(WeightBasedRoute {
                routes: Arc::new(RwLock::new(vec![WeightRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("http://httpbin.org/"),
                        try_file: None,
                        is_alive: Arc::new(RwLock::new(None)),
                        anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        })),
                    },
                    weight: 100,
                    index: Arc::new(AtomicIsize::new(0)),
                }])),
            }),
            health_check: Some(HealthCheckType::HttpGet(HttpHealthCheckParam {
                base_health_check_param: BaseHealthCheckParam {
                    timeout: 10,
                    interval: 3,
                },
                path: String::from("/get"),
            })),
            allow_deny_list: None,
            anomaly_detection: None,
            liveness_config: None,
            rewrite_headers: None,

            liveness_status: Arc::new(RwLock::new(LivenessStatus {
                current_liveness_count: 0,
            })),
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service_manager = ApiServiceManager {
            sender,
            service_config: ServiceConfig {
                key_str: None,
                server_type: crate::vojo::app_config::ServiceType::Https,
                cert_str: None,
                routes: vec![route],
            },
        };
        let uuid2 = Uuid::new_v4();
        let key = uuid2.to_string();

        TOKIO_RUNTIME.spawn(async move {
            GLOBAL_CONFIG_MAPPING.insert(key.clone(), api_service_manager);

            let mut health_check = HealthCheck::new();
            let res = health_check.do_health_check().await;
            assert!(res.is_ok());
            let res2 = health_check.do_health_check().await;
            assert!(res2.is_ok());
        });
        sleep(Duration::from_secs(10)).await;
    }
    #[tokio::test]
    async fn test_do_health_check_err1() {
        let id = Uuid::new_v4();
        let (sender, _receiver) = tokio::sync::mpsc::channel(10);
        let route = Route {
            host_name: None,
            route_id: id.to_string(),
            route_cluster: LoadbalancerStrategy::WeightBased(WeightBasedRoute {
                routes: Arc::new(RwLock::new(vec![WeightRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("http://127.0.0.1:9394/"),
                        try_file: None,
                        is_alive: Arc::new(RwLock::new(None)),
                        anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                            consecutive_5xx: 100,
                        })),
                    },
                    weight: 100,
                    index: Arc::new(AtomicIsize::new(0)),
                }])),
            }),
            health_check: Some(HealthCheckType::HttpGet(HttpHealthCheckParam {
                base_health_check_param: BaseHealthCheckParam {
                    timeout: 10,
                    interval: 3,
                },
                path: String::from("/get"),
            })),
            allow_deny_list: None,
            anomaly_detection: None,
            rewrite_headers: None,

            liveness_config: Some(LivenessConfig {
                min_liveness_count: 3,
            }),
            liveness_status: Arc::new(RwLock::new(LivenessStatus {
                current_liveness_count: 0,
            })),
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let api_service_manager = ApiServiceManager {
            sender,
            service_config: ServiceConfig {
                key_str: None,
                server_type: crate::vojo::app_config::ServiceType::Https,
                cert_str: None,
                routes: vec![route],
            },
        };
        let uuid2 = Uuid::new_v4();
        let key = uuid2.to_string();

        TOKIO_RUNTIME.spawn(async move {
            GLOBAL_CONFIG_MAPPING.insert(key.clone(), api_service_manager);

            let mut health_check = HealthCheck::new();
            let res = health_check.do_health_check().await;
            assert!(res.is_ok());
        });
        sleep(Duration::from_secs(10)).await;
    }
    #[test]

    fn test_do_http_health_check_error1() {
        let http_health_check_param = HttpHealthCheckParam {
            base_health_check_param: BaseHealthCheckParam {
                timeout: 10,
                interval: 10,
            },
            path: String::from("test"),
        };
        let id = Uuid::new_v4();

        let route = Route {
            host_name: None,
            route_id: id.to_string(),
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
                    weight: 100,
                    index: Arc::new(AtomicIsize::new(0)),
                }])),
            }),
            health_check: Some(HealthCheckType::HttpGet(HttpHealthCheckParam {
                base_health_check_param: BaseHealthCheckParam {
                    timeout: 10,
                    interval: 10,
                },
                path: String::from("value"),
            })),
            liveness_config: None,
            liveness_status: Arc::new(RwLock::new(LivenessStatus {
                current_liveness_count: 0,
            })),
            rewrite_headers: None,

            anomaly_detection: None,
            allow_deny_list: None,
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        TOKIO_RUNTIME.block_on(async {
            let result =
                do_http_health_check(http_health_check_param, route, 10, HealthCheckClient::new())
                    .await;
            assert!(result.is_ok());
        });
    }
    #[test]

    fn test_do_http_health_check_error2() {
        let http_health_check_param = HttpHealthCheckParam {
            base_health_check_param: BaseHealthCheckParam {
                timeout: 10,
                interval: 10,
            },
            path: String::from("test"),
        };
        let id = Uuid::new_v4();

        let route = Route {
            host_name: None,
            route_id: id.to_string(),
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
                    weight: 100,
                    index: Arc::new(AtomicIsize::new(0)),
                }])),
            }),
            health_check: Some(HealthCheckType::HttpGet(HttpHealthCheckParam {
                base_health_check_param: BaseHealthCheckParam {
                    timeout: 10,
                    interval: 10,
                },
                path: String::from("value"),
            })),
            anomaly_detection: None,
            allow_deny_list: None,
            authentication: None,
            liveness_config: None,
            rewrite_headers: None,

            liveness_status: Arc::new(RwLock::new(LivenessStatus {
                current_liveness_count: 0,
            })),
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        TOKIO_RUNTIME.block_on(async {
            let result =
                do_http_health_check(http_health_check_param, route, 10, HealthCheckClient::new())
                    .await;
            assert!(result.is_ok());
        });
    }
    #[test]

    fn test_do_http_health_check_ok1() {
        let http_health_check_param = HttpHealthCheckParam {
            base_health_check_param: BaseHealthCheckParam {
                timeout: 10,
                interval: 10,
            },
            path: String::from("test"),
        };
        let id = Uuid::new_v4();

        let route = Route {
            host_name: None,
            route_id: id.to_string(),
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
                    weight: 100,
                    index: Arc::new(AtomicIsize::new(0)),
                }])),
            }),
            health_check: Some(HealthCheckType::HttpGet(HttpHealthCheckParam {
                base_health_check_param: BaseHealthCheckParam {
                    timeout: 10,
                    interval: 10,
                },
                path: String::from("/"),
            })),
            liveness_status: Arc::new(RwLock::new(LivenessStatus {
                current_liveness_count: 0,
            })),
            anomaly_detection: None,
            allow_deny_list: None,
            authentication: None,
            rewrite_headers: None,

            liveness_config: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        TOKIO_RUNTIME.block_on(async {
            let result =
                do_http_health_check(http_health_check_param, route, 10, HealthCheckClient::new())
                    .await;
            assert!(result.is_ok());
        });
    }
}
