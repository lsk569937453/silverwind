use crate::vojo::app_config::Route;
use crate::vojo::app_error::AppError;
use crate::vojo::route::BaseRoute;
use async_trait::async_trait;
use http::HeaderMap;
use hyper::Uri;
use std::net::SocketAddr;
use std::path::Path;
use url::Url;
#[async_trait]
pub trait CheckTrait {
    async fn check_before_request(
        &self,
        mapping_key: String,
        headers: HeaderMap,
        uri: Uri,
        peer_addr: SocketAddr,
    ) -> Result<Option<CheckResult>, AppError>;
}
pub struct CommonCheckRequest;
impl CommonCheckRequest {
    pub fn new() -> Self {
        CommonCheckRequest {}
    }
}
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub request_path: String,
    pub route: Route,
    pub base_route: BaseRoute,
}

#[async_trait]
impl CheckTrait for CommonCheckRequest {
    async fn check_before_request(
        &self,
        mapping_key: String,
        headers: HeaderMap,
        uri: Uri,
        peer_addr: SocketAddr,
    ) -> Result<Option<CheckResult>, AppError> {
        let backend_path = uri
            .path_and_query()
            .ok_or(AppError(String::from("")))?
            .to_string();
        let app_config = GLOBAL_APP_CONFIG.lock().await;
        let api_service_manager = app_config
            .api_service_config
            .get(&mapping_key)
            .ok_or(AppError(format!(
                "Can not find the config mapping on the key {}!",
                mapping_key.clone()
            )))?
            .clone();
        let addr_string = peer_addr.ip().to_string();
        for item in api_service_manager.service_config.routes {
            let back_path_clone = backend_path.clone();
            let match_result = item.is_matched(back_path_clone, Some(headers.clone()))?;
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
            let endpoint = base_route.endpoint.clone();
            debug!("The endpoint is {}", endpoint);
            if endpoint.contains("http") {
                let host = Url::parse(endpoint.as_str()).map_err(|e| AppError(e.to_string()))?;
                let rest_path = match_result.unwrap();

                let request_path = host
                    .join(rest_path.as_str())
                    .map_err(|e| AppError(e.to_string()))?
                    .to_string();
                return Ok(Some(CheckResult {
                    request_path,
                    route: item,
                    base_route,
                }));
            } else {
                let path = Path::new(&endpoint);
                let rest_path = match_result.unwrap();
                let request_path = path.join(rest_path);
                return Ok(Some(CheckResult {
                    request_path: String::from(request_path.to_str().unwrap_or_default()),
                    route: item,
                    base_route,
                }));
            }
        }
        Ok(None)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_url_parse() {
        let host = Url::parse("http://127.0.0.1:8080");
        assert!(host.is_ok());
    }
}
