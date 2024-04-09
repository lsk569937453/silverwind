use crate::configuration_service::logger;
use crate::constants;
use crate::constants::common_constants::ENV_ACCESS_LOG;
use crate::constants::common_constants::ENV_ADMIN_PORT;
use crate::constants::common_constants::ENV_CONFIG_FILE_PATH;
use crate::constants::common_constants::ENV_DATABASE_URL;
use crate::constants::common_constants::TIMER_WAIT_SECONDS;
use crate::health_check::health_check_task::HealthCheck;
use crate::proxy::http1::http_proxy::HttpProxy;
use crate::proxy::http2::grpc_proxy::GrpcProxy;
use crate::proxy::tcp::tcp_proxy::TcpProxy;
use crate::vojo::api_service_manager::ApiServiceManager;
use crate::vojo::app_config::ServiceConfig;
use crate::vojo::app_config::{ApiService, AppConfig, ServiceType};
use crate::vojo::app_error::AppError;
use dashmap::DashMap;
use futures::FutureExt;
use lazy_static::lazy_static;
use log::Level;
use std::collections::HashMap;
use std::env;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::time::sleep;
lazy_static! {
    pub static ref GLOBAL_APP_CONFIG: Mutex<AppConfig> = Mutex::new(Default::default());
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
async fn update_mapping_from_global_appconfig() -> Result<(), AppError> {
    let rw_global_app_config = GLOBAL_APP_CONFIG.lock().await;
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
) -> Result<(), AppError> {
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
    } else if server_type == ServiceType::Tcp {
        let mut tcp_proxy = TcpProxy {
            port,
            mapping_key,
            channel,
        };
        tcp_proxy.start_proxy().await
    } else if server_type == ServiceType::Http2 {
        let mut grpc_proxy = GrpcProxy {
            port,
            mapping_key,
            channel,
        };
        grpc_proxy.start_proxy().await
    } else {
        let key_clone = mapping_key.clone();
        let service_config = GLOBAL_CONFIG_MAPPING
            .get(&key_clone)
            .unwrap()
            .service_config
            .clone();
        let pem_str = service_config.cert_str.unwrap();
        let key_str = service_config.key_str.unwrap();
        let mut grpc_proxy = GrpcProxy {
            port,
            mapping_key,
            channel,
        };
        grpc_proxy.start_tls_proxy(pem_str, key_str).await
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
async fn init_app_service_config() -> Result<(), AppError> {
    let rw_app_config_read = GLOBAL_APP_CONFIG.read().await;

    let config_file_path = rw_app_config_read.static_config.config_file_path.clone();
    if config_file_path.is_none() {
        return Ok(());
    }
    drop(rw_app_config_read);
    let file_path = config_file_path.unwrap().clone();
    info!("the config file is in{}", file_path.clone());
    let file = std::fs::File::open(file_path).map_err(|e| AppError(e.to_string()))?;
    let scrape_config: Vec<ApiService> = match serde_yaml::from_reader(file) {
        Ok(apiservices) => apiservices,
        Err(err) => return Err(AppError(err.to_string())),
    };
    let mut rw_app_config_write = GLOBAL_APP_CONFIG.write().await;

    let mut res = vec![];
    for item in scrape_config {
        res.push(item);
    }
    rw_app_config_write.api_service_config = res;
    Ok(())
}
