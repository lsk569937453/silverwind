use crate::health_check::timer::TaskPool;
use crate::proxy::http1::http_client::HttpClients;
use crate::vojo::app_config::ApiService;
use crate::vojo::app_config::AppConfig;
use crate::vojo::app_config::Route;
use crate::vojo::app_error::AppError;
use crate::vojo::health_check::HealthCheckType;
use crate::vojo::health_check::HttpHealthCheckParam;
use bytes::Bytes;
use futures;
use futures::future::join_all;
use futures::FutureExt;
use futures::Stream;
use http::Request;
use http::StatusCode;
use http_body_util::BodyExt;
use http_body_util::Full;
use openssl::sha;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio::time;
use url::Url;
#[derive(Clone)]
pub struct HealthCheckClient {
    pub http_clients: HttpClients,
}
impl HealthCheckClient {
    pub fn new() -> Self {
        HealthCheckClient {
            http_clients: HttpClients::new(false),
        }
    }
}
#[derive(Hash, Clone, Eq, PartialEq, Debug)]
pub struct TaskKey {
    pub route_id: String,
    pub api_service_id: String,
    pub health_check_type: HealthCheckType,
    pub endpoint_list: Vec<(String, String)>,
    pub min_liveness_count: i32,
}
impl TaskKey {
    pub fn new(
        route_id: String,
        api_service_id: String,
        health_check_type: HealthCheckType,
        endpoint_list: Vec<(String, String)>,
        min_liveness_count: i32,
    ) -> Self {
        TaskKey {
            route_id,
            api_service_id,
            health_check_type,
            endpoint_list,
            min_liveness_count,
        }
    }
}

pub struct HealthCheck {
    pub task_id_map: HashSet<TaskKey>,
    pub health_check_client: HealthCheckClient,
    pub current_id: Arc<AtomicU64>,
    pub shared_config: Arc<Mutex<AppConfig>>,
    pub task_pool: TaskPool,
}
impl HealthCheck {
    pub fn new(shared_config: Arc<Mutex<AppConfig>>) -> Self {
        HealthCheck {
            task_id_map: HashSet::new(),
            health_check_client: HealthCheckClient::new(),
            current_id: Arc::new(AtomicU64::new(0)),
            shared_config,
            task_pool: TaskPool::new(),
        }
    }
    pub async fn start_health_check_loop(&mut self) {
        let mut interval = time::interval(Duration::from_secs(5));

        loop {
            let async_result = std::panic::AssertUnwindSafe(self.do_health_check())
                .catch_unwind()
                .await;
            if async_result.is_err() {
                error!("start_health_check_loop catch panic successfully!");
            }
            interval.tick().await;
        }
    }

    async fn do_health_check(&mut self) -> Result<(), AppError> {
        let app_config = self.shared_config.lock().await;
        let apiservice_map = app_config.api_service_config.clone();
        drop(app_config);

        let mut route_map = HashMap::new();
        for (key, value) in apiservice_map {
            for route in value.service_config.routes {
                if route.health_check.is_some() && route.liveness_config.is_some() {
                    let endpoint_list = get_endpoint_list(route.clone());
                    let min_liveness_count =
                        route.liveness_config.clone().unwrap().min_liveness_count;
                    let task_key = TaskKey::new(
                        route.route_id.clone(),
                        key.clone(),
                        route.health_check.clone().unwrap(),
                        endpoint_list,
                        min_liveness_count,
                    );
                    route_map.insert(task_key, route);
                }
            }
        }

        // Remove tasks from task_id_map for routes not present in route_list
        self.task_id_map.retain(|task_key| {
            if !route_map.contains_key(task_key) {
                let res = self.task_pool.remove_task(task_key.route_id.clone());
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
            true
        });
        let old_map = self.task_id_map.clone();
        // For each route in route_list that is not in the old_map, submit a health check task

        for (key, value) in route_map {
            if !old_map.contains(&key) {
                let task_id = value.clone().route_id;
                let health_check_client = self.health_check_client.clone();
                let health_check_type = value.health_check.clone().unwrap();
                let route_list = get_endpoint_list(value.clone());
                let route_id = value.route_id.clone();
                let api_service_id = key.api_service_id.clone();
                let timeout_share = 20;
                let health_check_client_shared = health_check_client.clone();
                let shared_config = self.shared_config.clone();
                let task = move || {
                    let route_list = route_list.clone();
                    let route_id = route_id.clone();
                    let api_service_id = api_service_id.clone();
                    let health_check_client_shared = health_check_client_shared.clone();
                    let health_check_type_cloned = health_check_type.clone();

                    let shared_config = shared_config.clone();

                    async move {
                        let cc = match health_check_type_cloned {
                            HealthCheckType::HttpGet(http_health_check_param) => {
                                do_http_health_check(
                                    http_health_check_param,
                                    route_list,
                                    route_id,
                                    timeout_share,
                                    health_check_client_shared,
                                    shared_config,
                                    api_service_id,
                                )
                                .await
                            }
                            _ => Err(AppError("".to_string())),
                        };
                        cc
                    }
                };
                let _ = self.task_pool.submit_task(task_id, task, 5).await;
                self.task_id_map.insert(key.clone());
            }
        }
        Ok(())
    }
}
fn get_endpoint_list(mut route: Route) -> Vec<(String, String)> {
    let mut result = vec![];
    let base_route_list = route.route_cluster.get_all_route().unwrap_or(vec![]);
    for item in base_route_list {
        result.push((item.endpoint.clone(), item.base_route_id.clone()));
    }
    result
}
async fn do_http_health_check(
    http_health_check_param: HttpHealthCheckParam,
    route_list: Vec<(String, String)>,
    route_id: String,
    timeout_number: i32,
    http_health_check_client: HealthCheckClient,
    shared_config: Arc<Mutex<AppConfig>>,
    api_service_id: String,
) -> Result<(), AppError> {
    let http_client = http_health_check_client.http_clients.clone();
    let mut set = JoinSet::new();
    for (item, base_route_id) in route_list {
        let http_client_shared = http_client.clone();
        let host_option = Url::parse(item.as_str());
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
        let route_id_cloned = route_id.clone();
        let shared_config_cloned = shared_config.clone();
        let api_service_id_cloned = api_service_id.clone();
        let base_route_id_cloned = base_route_id.clone();
        info!(
            "health check url:{}",
            join_option.clone().unwrap().to_string()
        );
        let req = Request::builder()
            .uri(join_option.unwrap().to_string())
            .method("GET")
            .body(Full::new(Bytes::new()).boxed())
            .map_err(|err| AppError(err.to_string()))?;
        let task_with_timeout = http_client_shared
            .clone()
            .request_http(req, timeout_number as u64);
        set.spawn(async move {
            let res = task_with_timeout.await;
            (
                res,
                route_id_cloned,
                shared_config_cloned,
                api_service_id_cloned,
                base_route_id_cloned,
            )
        });
    }
    while let Some(response_result1) = set.join_next().await {
        if let Ok((response_result2, route_id, shared_config, api_service_id, base_route_id)) =
            response_result1
        {
            match response_result2 {
                Ok(Ok(t)) => {
                    if t.status() == StatusCode::OK {
                        info!("status is true");
                        let _ = update_status(
                            shared_config,
                            api_service_id,
                            route_id.clone(),
                            base_route_id,
                            true,
                        )
                        .await;
                        continue;
                    }
                }
                _ => {}
            };

            info!("status is false");
            let _ = update_status(
                shared_config,
                api_service_id,
                route_id,
                base_route_id,
                false,
            )
            .await;
        }
    }
    Ok(())
}
async fn update_status(
    shared_config: Arc<Mutex<AppConfig>>,
    api_service_id: String,
    route_id: String,
    base_route_id: String,
    status: bool,
) -> Result<(), AppError> {
    let mut shared_config_lock = shared_config.lock().await;
    let api_service = shared_config_lock
        .api_service_config
        .get_mut(&api_service_id)
        .ok_or(AppError("Can not get the api service config".to_string()))?;
    let routes = api_service
        .service_config
        .routes
        .iter_mut()
        .find(|item| item.route_id == route_id)
        .ok_or(AppError("Can not get the route config".to_string()))?;
    let mut baseroutes = routes.route_cluster.get_all_route()?;
    // Modify one of the BaseRoute objects (assuming you have access to the original objects)
    let base_route = baseroutes
        .iter_mut()
        .find(|route| route.base_route_id == base_route_id)
        .ok_or(AppError("Can not get the route config".to_string()))?;
    base_route.is_alive = Some(status);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::vojo::api_service_manager::ApiServiceManager;
    use crate::vojo::app_config::LivenessConfig;
    use crate::vojo::app_config::LivenessStatus;
    use crate::vojo::app_config::Matcher;
    use crate::vojo::app_config::ServiceConfig;
    use crate::vojo::app_config::StaticConfig;
    use crate::vojo::health_check::BaseHealthCheckParam;
    use crate::vojo::route::AnomalyDetectionStatus;
    use crate::vojo::route::LoadbalancerStrategy;
    use crate::vojo::route::{BaseRoute, WeightBasedRoute, WeightRoute};
    use std::sync::atomic::AtomicIsize;
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio::sync::RwLock;

    use uuid::Uuid;

    fn creata_appconfig() -> AppConfig {
        let id = Uuid::new_v4();
        let route = Route {
            host_name: None,
            route_id: id.to_string(),
            route_cluster: LoadbalancerStrategy::WeightBasedRoute(WeightBasedRoute {
                index: 0,
                offset: 0,
                routes: vec![WeightRoute {
                    base_route: BaseRoute {
                        endpoint: String::from("http://www.937453.xyz"),
                        try_file: None,
                        base_route_id: String::from(""),
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
            health_check: Some(HealthCheckType::HttpGet(HttpHealthCheckParam {
                base_health_check_param: BaseHealthCheckParam {
                    timeout: 0,
                    interval: 2,
                },
                path: String::from("/"),
            })),
            liveness_config: Some(LivenessConfig {
                min_liveness_count: 1,
            }),
            allow_deny_list: None,
            rewrite_headers: None,

            authentication: None,
            ratelimit: None,
            matcher: Some(Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            }),
        };
        let (sender, _) = mpsc::channel(1);
        let api_service = ApiService {
            listen_port: 9090,
            api_service_id: String::from("0"),
            sender: sender,
            service_config: ServiceConfig {
                server_type: crate::vojo::app_config::ServiceType::Http,
                cert_str: None,
                key_str: None,
                routes: vec![route],
            },
        };
        let mut hashmap = HashMap::new();
        hashmap.insert(String::from("a"), api_service);
        let app_config = AppConfig {
            api_service_config: hashmap,
            static_config: StaticConfig {
                access_log: None,
                database_url: None,
                admin_port: String::from("9394"),
                config_file_path: None,
            },
        };
        app_config
    }

    #[test]
    fn test_serde() {
        let app_config = creata_appconfig();
        let str = serde_json::to_string(&app_config).unwrap();
        println!("{}", str);
    }
    #[tokio::test]
    async fn test_submit_task_success1() {
        let app_config = creata_appconfig();
        let mut health_check = HealthCheck::new(Arc::new(Mutex::new(app_config)));
        let res = health_check.do_health_check().await;
        assert!(res.is_ok());
    }
    #[tokio::test]
    async fn test_submit_task_success2() {
        let app_config = creata_appconfig();
        let shared_app_config = Arc::new(Mutex::new(app_config));
        let mut health_check = HealthCheck::new(shared_app_config.clone());
        let mut res = health_check.do_health_check().await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(res.is_ok());
        let mut shared_app_config_lock = shared_app_config.lock().await;
        if let Some((_, value)) = shared_app_config_lock.api_service_config.iter_mut().next() {
            if let Some(route) = value.service_config.routes.first_mut() {
                route.liveness_config = Some(LivenessConfig {
                    min_liveness_count: 3,
                });
            } // value.service_config.routes.first().
        }
        drop(shared_app_config_lock);
        res = health_check.do_health_check().await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(res.is_ok());
        let shared_app_config_lock = shared_app_config.lock().await;
        let app_config = shared_app_config_lock.clone();
        let mut apiservices = app_config
            .api_service_config
            .values()
            .cloned()
            .collect::<Vec<ApiService>>();
        let first_element = apiservices.first_mut().unwrap();
        let baseroutes = first_element.service_config.routes[0]
            .route_cluster
            .get_all_route()
            .unwrap();
        let baseroute = baseroutes.first().unwrap();
        assert_eq!(baseroute.is_alive, Some(false));
    }
}
