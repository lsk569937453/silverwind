use crate::configuration_service::app_config_service::GLOBAL_CONFIG_MAPPING;
use crate::constants::constants::TIMER_WAIT_SECONDS;
use crate::proxy::http_proxy::Clients;
use crate::vojo::app_config::Route;
use crate::vojo::health_check::HealthCheckType;
use crate::vojo::health_check::HttpHealthCheckParam;
use delay_timer::prelude::*;

use futures::FutureExt;
use http::Request;
use http::StatusCode;
use hyper::Body;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use url::Url;

use std::time::Duration;

use std::sync::Arc;
use tokio::task::JoinSet;
use tokio::time::sleep;
use tokio::time::timeout;

#[derive(Clone)]
pub struct HealthCheckClient {
    pub http_clients: Clients,
}
impl HealthCheckClient {
    pub fn new() -> Self {
        HealthCheckClient {
            http_clients: Clients::new(),
        }
    }
}
#[derive(Hash, Clone, Eq, PartialEq, Debug)]
pub struct TaskKey {
    pub route_id: String,
    pub health_check_type: HealthCheckType,
}
impl TaskKey {
    pub fn new(route_id: String, health_check_type: HealthCheckType) -> Self {
        TaskKey {
            route_id: route_id,
            health_check_type: health_check_type,
        }
    }
}
pub struct HealthCheck {
    pub task_id_map: HashMap<TaskKey, u64>,
    pub delay_timer: DelayTimer,
    pub health_check_client: HealthCheckClient,
    pub current_id: Arc<AtomicU64>,
}
impl HealthCheck {
    pub fn new() -> Self {
        HealthCheck {
            task_id_map: HashMap::new(),
            delay_timer: DelayTimerBuilder::default().build(),
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
        let route_list = GLOBAL_CONFIG_MAPPING
            .iter()
            .flat_map(|item| item.service_config.routes.clone())
            .filter(|item| item.health_check.is_some())
            .map(|item| {
                (
                    TaskKey::new(
                        item.route_id.clone(),
                        item.health_check.clone().unwrap().clone(),
                    ),
                    item,
                )
            })
            .collect::<HashMap<TaskKey, Route>>();
        //delet the old task
        self.task_id_map.retain(|route_id, task_id| {
            if !route_list.contains_key(route_id) {
                let res = self.delay_timer.remove_task(task_id.clone());
                if let Err(err) = res {
                    error!(
                        "Health check task remove task error,the error is {}.",
                        err.to_string()
                    );
                    return true;
                } else {
                    return false;
                }
            }
            return true;
        });
        let old_map = self.task_id_map.clone();
        route_list
            .iter()
            .filter(|(task_key, _)| !old_map.contains_key(task_key.clone()))
            .for_each(|(task_key, route)| {
                let current_id = self.current_id.fetch_add(1, Ordering::SeqCst);
                let submit_task_result =
                    submit_task(current_id, route.clone(), self.health_check_client.clone());
                if let Ok(submit_result) = submit_task_result {
                    let res = self.delay_timer.insert_task(submit_result);
                    if let Ok(_task_instance_chain) = res {
                        self.task_id_map.insert(task_key.clone(), current_id);
                    }
                } else {
                    error!("Submit task error");
                }
            });

        Ok(())
    }
}

async fn do_http_health_check(
    http_health_check_param: HttpHealthCheckParam,
    mut route: Route,
    timeout_number: i32,
    http_health_check_client: HealthCheckClient,
) -> Result<(), anyhow::Error> {
    let route_list = route.route_cluster.get_all_route()?;
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
            .body(Body::empty())
            .unwrap();
        let task = http_client_shared.clone().request_http(req);
        let task_with_timeout = timeout(Duration::from_secs(timeout_number as u64), task);
        set.spawn(async {
            let res = task_with_timeout.await;
            (res, item)
        });
    }
    while let Some(response_result1) = set.join_next().await {
        if let Ok((response_result2, base_route)) = response_result1 {
            if let Ok(response_result3) = response_result2 {
                if let Ok(response_result4) = response_result3 {
                    if response_result4.status() == StatusCode::OK {
                        if let Err(err) = base_route
                            .update_health_check_status_with_ok(route.liveness_status.clone())
                        {
                            error!("Update status error,the error is :{}", err)
                        } else {
                            info!(
                                "Update the liveness of route-{} to ok succesfully!",
                                base_route.endpoint
                            )
                        }
                        continue;
                    }
                }
            }
            if let Some(current_liveness_config) = route.liveness_config.clone() {
                let update_result = base_route.update_health_check_status_with_fail(
                    route.liveness_status.clone(),
                    current_liveness_config,
                );
                if update_result.is_err() {
                    error!(
                        "Update status error,the error is :{}",
                        update_result.unwrap_err()
                    );
                } else {
                    info!(
                        "Update the liveness of route-{} to fail succesfully!",
                        base_route.endpoint.clone()
                    )
                }
            } else {
                error!(
                    "Can not update the route-{} to fail,as the liveness_status is empty!",
                    base_route.endpoint.clone()
                );
            }
        }
    }
    Ok(())
}
fn submit_task(
    task_id: u64,
    route: Route,
    health_check_clients: HealthCheckClient,
) -> Result<Task, anyhow::Error> {
    if let Some(health_check) = route.health_check.clone() {
        let mut task_builder = TaskBuilder::default();
        let base_param = health_check.clone().get_base_param();
        let timeout = base_param.clone().timeout;
        let task = move || {
            let route_share = route.clone();
            let timeout_share = timeout.clone();
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
                    HealthCheckType::Mysql(_) => Ok({}),
                    HealthCheckType::Redis(_) => Ok({}),
                }
            }
        };

        return task_builder
            .set_task_id(task_id)
            .set_frequency_repeated_by_seconds(base_param.interval as u64)
            .spawn_async_routine(task)
            .map_err(|err| anyhow!(err.to_string()));
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
    use crate::vojo::route::{BaseRoute, WeightBasedRoute, WeightRoute};
    use lazy_static::lazy_static;
    use std::sync::atomic::AtomicIsize;
    use std::sync::{Arc, RwLock};
    use tokio::runtime::{Builder, Runtime};
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
            route_cluster: Box::new(WeightBasedRoute {
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
            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let health_check_param = HealthCheckClient::new();
        let res = submit_task(0, route, health_check_param);
        assert_eq!(res.is_err(), true);
    }
    #[tokio::test]
    async fn test_submit_task_ok1() {
        let id = Uuid::new_v4();
        let route = Route {
            host_name: None,
            route_id: id.to_string(),
            route_cluster: Box::new(WeightBasedRoute {
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
        assert_eq!(res.is_ok(), true);
        let delay_timer = DelayTimerBuilder::default().build();
        let res = delay_timer.insert_task(res.unwrap());
        sleep(Duration::from_secs(2)).await;
        assert_eq!(res.is_ok(), true);
    }
    #[test]
    fn test_do_health_check_ok1() {
        let id = Uuid::new_v4();
        let (sender, _receiver) = tokio::sync::mpsc::channel(10);
        let route = Route {
            host_name: None,
            route_id: id.to_string(),
            route_cluster: Box::new(WeightBasedRoute {
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
            sender: sender,
            service_config: ServiceConfig {
                key_str: None,
                server_type: crate::vojo::app_config::ServiceType::HTTPS,
                cert_str: None,
                routes: vec![route],
            },
        };
        let uuid2 = Uuid::new_v4();
        let key = uuid2.to_string();

        TOKIO_RUNTIME.block_on(async move {
            GLOBAL_CONFIG_MAPPING.insert(key.clone().to_string(), api_service_manager);

            let mut health_check = HealthCheck::new();
            let res = health_check.do_health_check().await;
            assert_eq!(res.is_ok(), true);
            // let res2 = health_check.do_health_check().await;
            // assert_eq!(res2.is_ok(), true);
        });
    }

    #[tokio::test]
    async fn test_do_health_check_ok2() {
        let id = Uuid::new_v4();
        let (sender, _receiver) = tokio::sync::mpsc::channel(10);
        let route = Route {
            host_name: None,
            route_id: id.to_string(),
            route_cluster: Box::new(WeightBasedRoute {
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
            sender: sender,
            service_config: ServiceConfig {
                key_str: None,
                server_type: crate::vojo::app_config::ServiceType::HTTPS,
                cert_str: None,
                routes: vec![route],
            },
        };
        let uuid2 = Uuid::new_v4();
        let key = uuid2.to_string();

        TOKIO_RUNTIME.spawn(async move {
            GLOBAL_CONFIG_MAPPING.insert(key.clone().to_string(), api_service_manager);

            let mut health_check = HealthCheck::new();
            let res = health_check.do_health_check().await;
            assert_eq!(res.is_ok(), true);
            let res2 = health_check.do_health_check().await;
            assert_eq!(res2.is_ok(), true);
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
            route_cluster: Box::new(WeightBasedRoute {
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
            sender: sender,
            service_config: ServiceConfig {
                key_str: None,
                server_type: crate::vojo::app_config::ServiceType::HTTPS,
                cert_str: None,
                routes: vec![route],
            },
        };
        let uuid2 = Uuid::new_v4();
        let key = uuid2.to_string();

        TOKIO_RUNTIME.spawn(async move {
            GLOBAL_CONFIG_MAPPING.insert(key.clone().to_string(), api_service_manager);

            let mut health_check = HealthCheck::new();
            let res = health_check.do_health_check().await;
            assert_eq!(res.is_ok(), true);
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
            route_cluster: Box::new(WeightBasedRoute {
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
            assert_eq!(result.is_ok(), true);
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
            route_cluster: Box::new(WeightBasedRoute {
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
            assert_eq!(result.is_ok(), true);
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
            route_cluster: Box::new(WeightBasedRoute {
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
            assert_eq!(result.is_ok(), true);
        });
    }
}
