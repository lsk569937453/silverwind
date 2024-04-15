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
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio::time::sleep;
use url::Url;
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
    pub api_service_id: String,
    pub health_check_type: HealthCheckType,
    pub endpoint_list: Vec<String>,
    pub min_liveness_count: i32,
}
impl TaskKey {
    pub fn new(
        route_id: String,
        api_service_id: String,
        health_check_type: HealthCheckType,
        endpoint_list: Vec<String>,
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
        loop {
            let async_result = std::panic::AssertUnwindSafe(self.do_health_check())
                .catch_unwind()
                .await;
            if async_result.is_err() {
                error!("start_health_check_loop catch panic successfully!");
            }
            sleep(std::time::Duration::from_secs(5)).await;
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
            if !old_map.contains(&key).clone() {
                let task_id = value.clone().route_id;
                let health_check_client = self.health_check_client.clone();
                let health_check_type = value.health_check.clone().unwrap();
                let route_share = value.clone();
                let timeout_share = 20;
                let health_check_client_shared = health_check_client.clone();
                let health_check_type_shared = health_check_type.clone();
                let task = async move {
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
                };
                let submit_task_result = self.task_pool.submit_task(task_id, task).await;
                self.task_id_map.insert(key.clone());
            }
        }
        Ok(())
    }
}
fn get_endpoint_list(mut route: Route) -> Vec<String> {
    let mut result = vec![];
    let base_route_list = route.route_cluster.get_all_route().unwrap_or(vec![]);
    for item in base_route_list {
        result.push(item.endpoint);
    }
    result
}
async fn do_http_health_check(
    http_health_check_param: HttpHealthCheckParam,
    mut route: Route,
    timeout_number: i32,
    http_health_check_client: HealthCheckClient,
) -> Result<(), AppError> {
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
