use crate::configuration_service::logger;
use crate::constants;
use crate::constants::common_constants::ENV_ACCESS_LOG;
use crate::constants::common_constants::ENV_ADMIN_PORT;
use crate::constants::common_constants::ENV_CONFIG_FILE_PATH;
use crate::constants::common_constants::ENV_DATABASE_URL;
use crate::constants::common_constants::TIMER_WAIT_SECONDS;
use crate::health_check::health_check_task::HealthCheck;
use crate::proxy::tcp_proxy::TcpProxy;
use crate::proxy::HttpProxy;
use crate::vojo::api_service_manager::ApiServiceManager;
use crate::vojo::app_config::ServiceConfig;
use crate::vojo::app_config::{ApiService, AppConfig, ServiceType};
use crate::vojo::app_config_vistor::ApiServiceVistor;
use dashmap::DashMap;
use futures::FutureExt;
use lazy_static::lazy_static;
use log::Level;
use std::collections::HashMap;
use std::env;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tokio::time::sleep;
lazy_static! {
    pub static ref GLOBAL_APP_CONFIG: RwLock<AppConfig> = RwLock::new(Default::default());
    pub static ref GLOBAL_CONFIG_MAPPING: DashMap<String, ApiServiceManager> = Default::default();
}

pub async fn init() {
    init_static_config().await;
    match init_app_service_config().await {
        Ok(_) => info!("Initialize app service config successfully!"),
        Err(err) => error!("{}", err.to_string()),
    }
    tokio::task::spawn(async {
        sync_mapping_from_global_app_config().await;
    });
    tokio::task::spawn(async {
        let mut health_check = HealthCheck::new();
        health_check.start_health_check_loop().await;
    });
}
async fn sync_mapping_from_global_app_config() {
    loop {
        let async_result = std::panic::AssertUnwindSafe(update_mapping_from_global_appconfig())
            .catch_unwind()
            .await;
        if async_result.is_err() {
            error!("sync_mapping_from_global_app_config catch panic successfully!");
        }
        sleep(std::time::Duration::from_secs(TIMER_WAIT_SECONDS)).await;
    }
}
/**
*Key in Old Map:[1,2]
 Key in Current Map:[2,4,5]
*/
async fn update_mapping_from_global_appconfig() -> Result<(), anyhow::Error> {
    let rw_global_app_config = GLOBAL_APP_CONFIG
        .try_read()
        .map_err(|err| anyhow!(err.to_string()))?;
    let api_services = rw_global_app_config.api_service_config.clone();

    let new_item_hash = api_services
        .iter()
        .map(|s| {
            (
                format!("{}-{}", s.listen_port.clone(), s.service_config.server_type),
                s.service_config.clone(),
            )
        })
        .collect::<HashMap<String, ServiceConfig>>();

    let difference_ports = GLOBAL_CONFIG_MAPPING
        .iter()
        .map(|s| s.key().clone())
        .filter(|item| !new_item_hash.contains_key(item))
        .collect::<Vec<String>>();
    if log_enabled!(Level::Info) {
        debug!("The len of different ports is {}", difference_ports.len());
    }
    //delete the old mapping
    for item in difference_ports {
        let key = item.clone();
        let value = GLOBAL_CONFIG_MAPPING.get(&key).unwrap().sender.clone();
        match value.send(()).await {
            Ok(_) => info!("close the socket on the port {}", key),
            Err(err) => {
                error!(
                    "Cause error when closing the socket,the key is {},the error is {}.",
                    key,
                    err.to_string()
                )
            }
        };
        GLOBAL_CONFIG_MAPPING.remove(&key);
    }
    //add the new mapping and update the old
    for (key, value) in new_item_hash {
        //update
        if GLOBAL_CONFIG_MAPPING.contains_key(&key) {
            let mut ref_value = GLOBAL_CONFIG_MAPPING.get(&key).unwrap().clone();
            ref_value.service_config = value.clone();
            GLOBAL_CONFIG_MAPPING.insert(key.clone(), ref_value);
            //add
        } else {
            let (sender, receiver) = tokio::sync::mpsc::channel(10);
            GLOBAL_CONFIG_MAPPING.insert(
                key.clone(),
                ApiServiceManager {
                    service_config: value.clone(),
                    sender,
                },
            );
            let item_list: Vec<&str> = key.split('-').collect();
            let port_str = item_list.first().unwrap();
            let port: i32 = port_str.parse().unwrap();

            tokio::task::spawn(async move {
                if let Err(err) = start_proxy(port, receiver, value.server_type, key.clone()).await
                {
                    error!("{}", err.to_string());
                }
            });
        }
    }

    Ok(())
}
pub async fn start_proxy(
    port: i32,
    channel: mpsc::Receiver<()>,
    server_type: ServiceType,
    mapping_key: String,
) -> Result<(), anyhow::Error> {
    if server_type == ServiceType::Http {
        let mut http_proxy = HttpProxy {
            port,
            channel,
            mapping_key: mapping_key.clone(),
        };
        http_proxy.start_http_server().await
    } else if server_type == ServiceType::Https {
        let key_clone = mapping_key.clone();
        let service_config = GLOBAL_CONFIG_MAPPING
            .get(&key_clone)
            .unwrap()
            .service_config
            .clone();
        let pem_str = service_config.cert_str.unwrap();
        let key_str = service_config.key_str.unwrap();
        let mut http_proxy = HttpProxy {
            port,
            channel,
            mapping_key: mapping_key.clone(),
        };
        http_proxy.start_https_server(pem_str, key_str).await
    } else {
        let mut tcp_proxy = TcpProxy {
            port,
            mapping_key,
            channel,
        };
        tcp_proxy.start_proxy().await
    }
}
async fn init_static_config() {
    let database_url_result = env::var(ENV_DATABASE_URL);
    let api_port = env::var(ENV_ADMIN_PORT).unwrap_or(String::from(
        constants::common_constants::DEFAULT_ADMIN_PORT,
    ));
    let access_log_result = env::var(ENV_ACCESS_LOG);
    let config_file_path_result = env::var(ENV_CONFIG_FILE_PATH);

    let mut global_app_config = GLOBAL_APP_CONFIG.write().await;

    if let Ok(database_url) = database_url_result {
        global_app_config.static_config.database_url = Some(database_url);
    }
    global_app_config.static_config.admin_port = api_port.clone();

    logger::start_logger();

    if let Ok(access_log) = access_log_result {
        global_app_config.static_config.access_log = Some(access_log);
    }

    if let Ok(config_file_path) = config_file_path_result {
        global_app_config.static_config.config_file_path = Some(config_file_path);
    }
}
async fn init_app_service_config() -> Result<(), anyhow::Error> {
    let rw_app_config_read = GLOBAL_APP_CONFIG.read().await;

    let config_file_path = rw_app_config_read.static_config.config_file_path.clone();
    if config_file_path.is_none() {
        return Ok(());
    }
    drop(rw_app_config_read);
    let file_path = config_file_path.unwrap().clone();
    info!("the config file is in{}", file_path.clone());
    let file = std::fs::File::open(file_path)?;
    let scrape_config: Vec<ApiServiceVistor> = match serde_yaml::from_reader(file) {
        Ok(apiservices) => apiservices,
        Err(err) => return Err(anyhow!(err.to_string())),
    };
    let mut rw_app_config_write = GLOBAL_APP_CONFIG.write().await;

    let mut res = vec![];
    for item in scrape_config {
        res.push(ApiService::from(item).await?);
    }
    rw_app_config_write.api_service_config = res;
    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::vojo::app_config::LivenessStatus;
    use crate::vojo::app_config::Route;
    use crate::vojo::route::AnomalyDetectionStatus;
    use crate::vojo::route::{BaseRoute, LoadbalancerStrategy, RandomBaseRoute, RandomRoute};
    use serial_test::serial;
    use std::sync::Arc;
    use tokio::runtime::{Builder, Runtime};
    use tokio::sync::RwLock;
    lazy_static! {
        pub static ref TOKIO_RUNTIME: Runtime = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("my-custom-name")
            .thread_stack_size(3 * 1024 * 1024)
            .enable_all()
            .build()
            .unwrap();
    }
    async fn before_test() {
        let mut app_config = GLOBAL_APP_CONFIG.write().await;
        *app_config = Default::default();
        GLOBAL_CONFIG_MAPPING.clear();
        env::remove_var("DATABASE_URL");
        env::remove_var("ADMIN_PORT");
        env::remove_var("ACCESS_LOG");
        env::remove_var("CONFIG_FILE_PATH");
    }
    #[test]
    #[serial("test")]
    fn test_init_static_config_default() {
        TOKIO_RUNTIME.block_on(async move {
            before_test().await;
            init_static_config().await;
            let current = GLOBAL_APP_CONFIG.read().await;
            assert_eq!(current.static_config.access_log, None);
            assert_eq!(current.static_config.admin_port, String::from("8870"));
            assert_eq!(current.static_config.database_url, None);
            assert_eq!(current.static_config.config_file_path, None);
        });
    }
    #[test]
    #[serial("test")]
    fn test_init_static_config_from_env() {
        TOKIO_RUNTIME.block_on(async move {
            before_test().await;

            let database_url = "database_url";
            let port = 3360;
            let access_log = "/log/test.log";
            let config_path = "/root/config/config.yaml";

            env::set_var("DATABASE_URL", database_url);
            env::set_var("ADMIN_PORT", port.to_string());
            env::set_var("ACCESS_LOG", access_log);
            env::set_var("CONFIG_FILE_PATH", config_path);
            init_static_config().await;
            let current = GLOBAL_APP_CONFIG.read().await;
            assert_eq!(
                current.static_config.access_log,
                Some(String::from(access_log))
            );
            assert_eq!(current.static_config.admin_port, port.to_string());
            assert_eq!(
                current.static_config.access_log,
                Some(String::from(access_log))
            );
            assert_eq!(
                current.static_config.config_file_path,
                Some(String::from(config_path))
            );
        });
    }
    #[test]
    #[serial("test")]
    fn test_init_app_service_config_from_file() {
        TOKIO_RUNTIME.block_on(async move {
            before_test().await;
            let current_dir = env::current_dir()
                .unwrap()
                .join("config")
                .join("app_config.yaml");
            println!("{}", String::from(current_dir.to_str().unwrap()));
            env::set_var("CONFIG_FILE_PATH", current_dir);
            init_static_config().await;
            let res = init_app_service_config().await;
            assert!(res.is_ok());
            let app_config = GLOBAL_APP_CONFIG.read().await.clone();
            let api_services = app_config.api_service_config;
            assert!(api_services.len() <= 5);
            let api_service = api_services.first().cloned().unwrap();
            assert_eq!(api_service.listen_port, 4486);
            let api_service_routes = api_service.service_config.routes.first().cloned().unwrap();
            assert_eq!(api_service_routes.matcher.clone().unwrap().prefix, "/");
            assert_eq!(api_service_routes.matcher.unwrap().prefix_rewrite, "ssss");
        });
    }
    #[test]
    #[serial("test")]
    fn test_update_mapping_from_global_appconfig_with_default() {
        TOKIO_RUNTIME.block_on(async move {
            before_test().await;
            init_static_config().await;
            let res_init_app_service_config = init_app_service_config().await;
            assert!(res_init_app_service_config.is_ok());
            let res_update_config_mapping = update_mapping_from_global_appconfig().await;
            assert!(res_update_config_mapping.is_ok());
            assert!(GLOBAL_CONFIG_MAPPING.len() < 4);
        });
    }
    #[test]
    #[serial("test")]
    fn test_update_mapping_from_global_appconfig_with_routes() {
        TOKIO_RUNTIME.block_on(async {
            before_test().await;
            let current_dir = env::current_dir()
                .unwrap()
                .join("config")
                .join("app_config.yaml");
            println!("{}", String::from(current_dir.to_str().unwrap()));

            env::set_var("CONFIG_FILE_PATH", current_dir);
            init_static_config().await;
            let res_init_app_service_config = init_app_service_config().await;
            assert!(res_init_app_service_config.is_ok());
            let _res_update_mapping_from_global_appconfig =
                update_mapping_from_global_appconfig().await;
            // assert_eq!(res_update_mapping_from_global_appconfig.is_ok(), true);
            assert!(GLOBAL_CONFIG_MAPPING.len() <= 5);
            let api_service_manager_list = GLOBAL_CONFIG_MAPPING
                .iter()
                .map(|s| s.to_owned())
                .collect::<Vec<ApiServiceManager>>();
            assert!(api_service_manager_list.len() <= 5);
            let api_service_manager = api_service_manager_list.first().unwrap();
            let routes = api_service_manager.service_config.routes.first().unwrap();
            assert_eq!(routes.matcher.clone().unwrap().prefix, "/");
        });
    }
    #[test]
    #[serial("test")]
    fn test_start_https_proxy_ok() {
        let private_key_path = env::current_dir()
            .unwrap()
            .join("config")
            .join("test_key.pem");
        let private_key = std::fs::read_to_string(private_key_path).unwrap();

        let certificate_path = env::current_dir()
            .unwrap()
            .join("config")
            .join("test_cert.pem");
        let certificate = std::fs::read_to_string(certificate_path).unwrap();

        let route = LoadbalancerStrategy::Random(RandomRoute {
            routes: vec![RandomBaseRoute {
                base_route: BaseRoute {
                    endpoint: String::from("httpbin.org:80"),
                    try_file: None,
                    is_alive: Arc::new(RwLock::new(None)),
                    anomaly_detection_status: Arc::new(RwLock::new(AnomalyDetectionStatus {
                        consecutive_5xx: 100,
                    })),
                },
            }],
        });
        let (sender, receiver) = tokio::sync::mpsc::channel(10);

        let api_service_manager = ApiServiceManager {
            sender,
            service_config: ServiceConfig {
                key_str: Some(private_key),
                server_type: crate::vojo::app_config::ServiceType::Https,
                cert_str: Some(certificate),
                routes: vec![Route {
                    host_name: None,
                    route_id: crate::utils::uuid::get_uuid(),
                    matcher: Default::default(),
                    route_cluster: route,
                    allow_deny_list: None,
                    authentication: None,
                    ratelimit: None,
                    health_check: None,
                    anomaly_detection: None,
                    liveness_config: None,
                    liveness_status: Arc::new(RwLock::new(LivenessStatus {
                        current_liveness_count: 0,
                    })),
                }],
            },
        };
        GLOBAL_CONFIG_MAPPING.insert(String::from("test"), api_service_manager);
        TOKIO_RUNTIME.spawn(async {
            let _result =
                start_proxy(2256, receiver, ServiceType::Https, String::from("test")).await;
        });
    }
}
