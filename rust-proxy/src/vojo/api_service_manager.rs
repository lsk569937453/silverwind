use std::collections::HashMap;
use std::iter::Map;

use crate::vojo::app_config::Route;
use crate::vojo::app_config::ServiceConfig;
use crate::vojo::app_config::ServiceType;
use tokio::sync::mpsc;

use super::route;
#[derive(Clone)]
pub struct ApiServiceManager<'a> {
    pub service_config: NewServiceConfig<'a>,
    pub sender: mpsc::Sender<()>,
}
impl<'a> ApiServiceManager<'a> {
    pub fn update_routes(&mut self, new_service_config: ServiceConfig) {
        let mut source_config = self.service_config;
        source_config.server_type = new_service_config.server_type;
        source_config.cert_str = new_service_config.cert_str;
        let source_routes = source_config
            .routes
            .into_iter()
            .map(|item| (item.route_id.clone(), item))
            .collect::<HashMap<String, &Route>>();
        new_service_config.routes.iter().for_each(|item| {
            source_config.routes.push(item);
        });
    }
}
#[derive(Debug, Clone, Default)]
pub struct NewServiceConfig<'a> {
    pub server_type: ServiceType,
    pub cert_str: Option<String>,
    pub key_str: Option<String>,
    pub routes: Vec<&'a Route>,
}
impl<'a> NewServiceConfig<'a> {
    pub fn clone_from(new_service_config: ServiceConfig) -> Self {
        let routes = new_service_config
            .routes
            .iter()
            .map(|item| item)
            .collect::<Vec<&Route>>();
        NewServiceConfig {
            server_type: new_service_config.server_type,
            cert_str: new_service_config.cert_str,
            key_str: new_service_config.key_str,
            routes: routes,
        }
    }
}
