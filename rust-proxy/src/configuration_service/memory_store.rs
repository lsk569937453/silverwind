use crate::configuration_service::configuration_service::StoreStrategy;
use crate::vojo::app_config::AppConfig;
use lazy_static::lazy_static;
use std::sync::Mutex;

lazy_static! {
    pub static ref GLOBAL_APPCONFIG: Mutex<Option<AppConfig>> = Mutex::new(None);
}

struct MemoryStore;

impl StoreStrategy for MemoryStore {
    fn save_config(&self, app_config: AppConfig) {
        let mut option_app_config = GLOBAL_APPCONFIG.lock().unwrap();
        *option_app_config = Some(app_config);
    }
    fn get_config(&self) -> Option<AppConfig> {
        let option_app_config = GLOBAL_APPCONFIG.lock().unwrap();
        return option_app_config.clone();
    }
}
