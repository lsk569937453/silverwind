use crate::constants;
use crate::proxy::HttpProxy;
use crate::vojo::api_service_manager::ApiServiceManager;
use crate::vojo::app_config::{ApiService, AppConfig, Route};
use dashmap::DashMap;
use lazy_static::lazy_static;
use std::collections::{HashMap, HashSet};
use std::env;
use std::f32::consts::E;
use std::panic;
use std::sync::RwLock;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use tokio::sync::mpsc;
lazy_static! {
    pub static ref GLOBAL_APP_CONFIG: RwLock<AppConfig> = RwLock::new(Default::default());
    pub static ref GLOBAL_CONFIG_MAPPING: DashMap<i32, ApiServiceManager> = Default::default();
}

pub fn init() {
    init_static_config();
    init_app_service_config();
    tokio::task::spawn_blocking(move || {
        sync_mapping_from_global_app_config();
    });
}
fn sync_mapping_from_global_app_config() {
    loop {
        let result_panic = panic::catch_unwind(|| match update_mapping_from_global_appconfig() {
            Ok(()) => debug!("sync_mapping_from_global_app_config is ok"),
            Err(err) => error!("connect_with_database is error,the error is :{}", err),
        });
        if result_panic.is_err() {
            error!("caught panic!");
        }
        sleep(std::time::Duration::from_secs(5));
    }
}
/**
*Key in Old Map:[1,2]
 Key in Current Map:[2,4,5]
*/
fn update_mapping_from_global_appconfig() -> Result<(), anyhow::Error> {
    // let item_set: HashSet<_> = .re.iter().collect();
    // let difference: Vec<_> = new_items
    //     .into_iter()
    //     .filter(|item| !item_set.contains(item))
    //     .collect();

    let rw_global_app_config = GLOBAL_APP_CONFIG.read().unwrap();
    let api_services = rw_global_app_config.api_service_config.clone();
    let new_item_hash = api_services
        .iter()
        .map(|s| (s.listen_port.clone(), s.routes.clone()))
        .collect::<HashMap<i32, Vec<Route>>>();

    let difference_ports = GLOBAL_CONFIG_MAPPING
        .iter()
        .map(|s| *s.key())
        .filter(|item| !new_item_hash.contains_key(item))
        .collect::<Vec<i32>>();
    debug!("the len of different is {}", difference_ports.len());

    //delete the old mapping
    for item in difference_ports {
        let key = item.clone();
        let value = GLOBAL_CONFIG_MAPPING.get(&key).unwrap().sender.clone();
        tokio::task::spawn(async move {
            match value.clone().send(()).await {
                Ok(_) => info!("close socket success"),
                Err(err) => error!("{}", err.to_string()),
            }
        });

        GLOBAL_CONFIG_MAPPING.remove(&key);
    }
    //add the new mapping and update the old
    for (key, value) in new_item_hash {
        //update
        if GLOBAL_CONFIG_MAPPING.contains_key(&key) {
            let mut ref_value = GLOBAL_CONFIG_MAPPING.get(&key).unwrap().clone();
            ref_value.routes = Vec::from(value);
            GLOBAL_CONFIG_MAPPING.insert(key.clone(), ref_value);
            // Vec::from(value);
            //add
        } else {
            let (sender, mut receiver) = tokio::sync::mpsc::channel(10);
            GLOBAL_CONFIG_MAPPING.insert(
                key.clone(),
                ApiServiceManager {
                    routes: Vec::from(value.clone()),
                    sender: sender,
                },
            );
            let api_service = ApiService {
                listen_port: key.clone(),
                routes: Vec::from(value.clone()),
            };
            tokio::task::spawn(async move { start_proxy(key.clone(), receiver).await });
        }
    }

    Ok(())
}
pub async fn start_proxy(port: i32, channel: mpsc::Receiver<()>) {
    let mut http_proxy = HttpProxy {
        port: port,
        channel: channel,
    };
    http_proxy.start().await;
}
fn init_static_config() {
    let database_url_result = env::var("DATABASE_URL");
    let api_port_result = env::var("API_PORT");
    let access_log_result = env::var("ACCESS_LOG");
    let config_file_path_result = env::var("CONFIG_FILE_PATH");

    let mut global_app_config = GLOBAL_APP_CONFIG.write().unwrap();

    if database_url_result.is_ok() {
        (*global_app_config).static_config.database_url = Some(database_url_result.unwrap());
    }

    let mut api_port = String::new();
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
fn init_app_service_config() {
    let rw_app_config_read = GLOBAL_APP_CONFIG.read().unwrap();
    let mut config_file_path = rw_app_config_read.static_config.config_file_path.clone();
    if config_file_path.is_none() {
        return;
    }
    let file = std::fs::File::open(config_file_path.unwrap()).expect("Could not open file.");
    let scrape_config: Vec<ApiService> =
        serde_yaml::from_reader(file).expect("Could not read values.");
    let mut rw_app_config_write = GLOBAL_APP_CONFIG.write().unwrap();
    (*rw_app_config_write).api_service_config = scrape_config;
}
mod tests {
    use super::*;
    #[test]
    fn test_current_directory() {
        let cwd = env::current_dir().unwrap();
        let path: String = String::from(cwd.to_string_lossy());
        println!("{}", path);
    }
}
