use crate::vojo::app_config::ServiceConfig;
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct ApiServiceManager {
    pub service_config: ServiceConfig,
    pub sender: mpsc::Sender<()>,
}
