use crate::configuration_service::app_config_service::GLOBAL_CONFIG_MAPPING;
use async_trait::async_trait;
use http::HeaderMap;
use hyper::Uri;
use std::net::SocketAddr;
use url::Url;

#[async_trait]
pub trait CheckTrait {
    async fn check_before_request(
        &self,
        mapping_key: String,
        headers: HeaderMap,
        uri: Uri,
        peer_addr: SocketAddr,
    ) -> Result<Option<String>, anyhow::Error>;
}
pub struct CommonCheckRequest;
impl CommonCheckRequest {
    pub fn new() -> Self {
        CommonCheckRequest {}
    }
}
#[async_trait]
impl CheckTrait for CommonCheckRequest {
    async fn check_before_request(
        &self,
        mapping_key: String,
        headers: HeaderMap,
        uri: Uri,
        peer_addr: SocketAddr,
    ) -> Result<Option<String>, anyhow::Error> {
        let backend_path = uri.path();
        let api_service_manager = GLOBAL_CONFIG_MAPPING
            .get(&mapping_key)
            .ok_or(anyhow!(format!(
                "Can not find the config mapping on the key {}!",
                mapping_key.clone()
            )))?
            .clone();
        let addr_string = peer_addr.ip().to_string();
        for item in api_service_manager.service_config.routes {
            let match_result = item.is_matched(backend_path, Some(headers.clone()))?;
            if match_result.clone().is_none() {
                continue;
            }
            let is_allowed = item
                .is_allowed(addr_string.clone(), Some(headers.clone()))
                .await?;
            if !is_allowed {
                return Ok(None);
            }
            let base_route = item
                .route_cluster
                .clone()
                .get_route(headers.clone())
                .await?;
            let endpoint = base_route.endpoint;
            let host = Url::parse(endpoint.as_str())?;
            let rest_path = match_result.unwrap();

            let request_path = host.join(rest_path.as_str())?.to_string();
            return Ok(Some(request_path));
        }
        Ok(None)
    }
}
