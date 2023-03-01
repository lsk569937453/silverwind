use crate::vojo::app_config::AppConfig;

pub trait StoreStrategy {
    fn save_config(&self, app_config: AppConfig);
    fn get_config(&self) -> Option<AppConfig>;
}
struct ConfigurationService<T: StoreStrategy> {
    store_strategy: T,
}

impl<T: StoreStrategy> ConfigurationService<T> {
    pub fn new(store_strategy: T) -> Self {
        Self { store_strategy }
    }

    pub fn save_config(&self, app_config: AppConfig) {
        self.store_strategy.save_config(app_config);
    }
    pub fn get_config(&self) -> Option<AppConfig> {
        self.store_strategy.get_config()
    }
}
