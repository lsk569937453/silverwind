use crate::constants;
use crate::proxy::tcp_proxy::TcpProxy;
use crate::proxy::HttpProxy;
use crate::vojo::api_service_manager::ApiServiceManager;
use crate::vojo::app_config::ServiceConfig;
use crate::vojo::app_config::{ApiService, AppConfig, ServiceType};
use dashmap::DashMap;
use futures::FutureExt;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::env;
use tokio::runtime::Handle;
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
    tokio::task::spawn_blocking(move || {
        Handle::current().block_on(async {
            sync_mapping_from_global_app_config().await;
        })
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
        sleep(std::time::Duration::from_secs(5)).await;
    }
}
/**
*Key in Old Map:[1,2]
 Key in Current Map:[2,4,5]
*/
async fn update_mapping_from_global_appconfig() -> Result<(), anyhow::Error> {
    let rw_global_app_config = GLOBAL_APP_CONFIG.try_read();
    let api_services = rw_global_app_config.unwrap().api_service_config.clone();

    let new_item_hash = api_services
        .iter()
        .map(|s| {
            (
                format!(
                    "{}-{}",
                    s.listen_port.clone(),
                    s.service_config.server_type.to_string()
                ),
                s.service_config.clone(),
            )
        })
        .collect::<HashMap<String, ServiceConfig>>();

    let difference_ports = GLOBAL_CONFIG_MAPPING
        .iter()
        .map(|s| s.key().clone())
        .filter(|item| !new_item_hash.contains_key(item))
        .collect::<Vec<String>>();
    debug!("The len of different ports is {}", difference_ports.len());
    //delete the old mapping
    for item in difference_ports {
        let key = item.clone();
        let value = GLOBAL_CONFIG_MAPPING.get(&key).unwrap().sender.clone();
        match value.send(()).await {
            Ok(_) => info!("close the socket on the port {}", key),
            Err(err) => error!("{}", err.to_string()),
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
                    sender: sender,
                },
            );
            let item_list: Vec<&str> = key.split("-").collect();
            let port_str = item_list.first().unwrap();
            let port: i32 = port_str.parse().unwrap();

            tokio::task::spawn(async move {
                start_proxy(port.clone(), receiver, value.server_type, key.clone()).await
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
) {
    if server_type == ServiceType::HTTP {
        let mut http_proxy = HttpProxy {
            port: port,
            channel: channel,
            mapping_key: mapping_key.clone(),
        };
        http_proxy.start_http_server().await;
    } else if server_type == ServiceType::HTTPS {
        let key_clone = mapping_key.clone();
        let service_config = GLOBAL_CONFIG_MAPPING
            .get(&key_clone)
            .unwrap()
            .service_config
            .clone();
        let pem_str = service_config.cert_str.unwrap();
        let key_str = service_config.key_str.unwrap();
        let mut http_proxy = HttpProxy {
            port: port,
            channel: channel,
            mapping_key: mapping_key.clone(),
        };
        http_proxy.start_https_server(pem_str, key_str).await;
    } else {
        let mut tcp_proxy = TcpProxy {
            port: port,
            mapping_key: mapping_key,
            channel: channel,
        };
        tcp_proxy.start_proxy().await
    }
}
async fn init_static_config() {
    let database_url_result = env::var("DATABASE_URL");
    let api_port_result = env::var("API_PORT");
    let access_log_result = env::var("ACCESS_LOG");
    let config_file_path_result = env::var("CONFIG_FILE_PATH");

    let mut global_app_config = GLOBAL_APP_CONFIG.write().await;

    if database_url_result.is_ok() {
        (*global_app_config).static_config.database_url = Some(database_url_result.unwrap());
    }

    let api_port;
    if api_port_result.is_ok() {
        api_port = api_port_result.clone().unwrap();
    } else {
        api_port = String::from(constants::constants::DEFAULT_API_PORT);
    }
    global_app_config.static_config.api_port = api_port.clone();
    env::set_var("ROCKET_PORT", api_port);

    if access_log_result.is_ok() {
        (*global_app_config).static_config.access_log = Some(access_log_result.unwrap());
    }

    if config_file_path_result.is_ok() {
        (*global_app_config).static_config.config_file_path =
            Some(config_file_path_result.unwrap());
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
    let file = match std::fs::File::open(file_path) {
        Ok(file) => file,
        Err(err) => return Err(anyhow!(err.to_string())),
    };
    let scrape_config: Vec<ApiService> = match serde_yaml::from_reader(file) {
        Ok(apiservices) => apiservices,
        Err(err) => return Err(anyhow!(err.to_string())),
    };
    let mut rw_app_config_write = GLOBAL_APP_CONFIG.write().await;

    (*rw_app_config_write).api_service_config = scrape_config;
    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;
    use serial_test::serial;
    use tokio::runtime::{Builder, Runtime};

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
        env::remove_var("API_PORT");
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
            assert_eq!(current.static_config.api_port, String::from("8870"));
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
            env::set_var("API_PORT", port.to_string());
            env::set_var("ACCESS_LOG", access_log);
            env::set_var("CONFIG_FILE_PATH", config_path);
            init_static_config().await;
            let current = GLOBAL_APP_CONFIG.read().await;
            assert_eq!(
                current.static_config.access_log,
                Some(String::from(access_log))
            );
            assert_eq!(
                current.static_config.api_port,
                String::from(port.to_string())
            );
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
            assert_eq!(res.is_ok(), true);
            let app_config = GLOBAL_APP_CONFIG.read().await.clone();
            let api_services = app_config.api_service_config.clone();
            assert_eq!(api_services.len(), 2);
            let api_service = api_services.first().cloned().unwrap();
            assert_eq!(api_service.listen_port, 4486);
            let api_service_routes = api_service.service_config.routes.first().cloned().unwrap();
            assert_eq!(api_service_routes.matcher.prefix, "/");
            assert_eq!(api_service_routes.matcher.prefix_rewrite, "ssss");
        });
    }
    #[test]
    #[serial("test")]
    fn test_update_mapping_from_global_appconfig_with_default() {
        TOKIO_RUNTIME.block_on(async move {
            before_test().await;
            init_static_config().await;
            let res_init_app_service_config = init_app_service_config().await;
            assert_eq!(res_init_app_service_config.is_err(), false);
            let res_update_config_mapping = update_mapping_from_global_appconfig().await;
            assert_eq!(res_update_config_mapping.is_err(), false);
            assert_eq!(GLOBAL_CONFIG_MAPPING.len(), 0);
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
            assert_eq!(res_init_app_service_config.is_err(), false);

            let res_update_mapping_from_global_appconfig =
                update_mapping_from_global_appconfig().await;
            assert_eq!(res_update_mapping_from_global_appconfig.is_ok(), true);
            assert_eq!(GLOBAL_CONFIG_MAPPING.len(), 2);
            let api_service_manager_list = GLOBAL_CONFIG_MAPPING
                .iter()
                .map(|s| s.to_owned())
                .collect::<Vec<ApiServiceManager>>();
            assert_eq!(api_service_manager_list.len(), 2);
            let api_service_manager = api_service_manager_list.first().unwrap();
            let routes = api_service_manager.service_config.routes.first().unwrap();
            assert_eq!(routes.matcher.prefix, "/");
        });
    }
}
