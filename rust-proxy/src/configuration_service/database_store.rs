use crate::configuration_service::configuration_service::StoreStrategy;
use crate::vojo::app_config::AppConfig;
struct DatabaseStore {}

impl StoreStrategy for DatabaseStore {
    fn save_config(&self, app_config: AppConfig) {}
    fn get_config(&self) -> Option<AppConfig> {
        None
    }
}
