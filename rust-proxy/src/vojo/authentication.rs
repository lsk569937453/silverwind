use base64::{engine::general_purpose, Engine as _};
use core::fmt::Debug;
use dyn_clone::DynClone;
use http::HeaderMap;
use http::HeaderValue;

use serde::{Deserialize, Serialize};
use std::any::Any;

use super::app_error::AppError;

#[typetag::serde(tag = "type")]
pub trait AuthenticationStrategy: Sync + Send + DynClone {
    fn check_authentication(&mut self, headers: HeaderMap<HeaderValue>) -> Result<bool, AppError>;

    fn get_debug(&self) -> String {
        String::from("debug")
    }
    fn as_any(&self) -> &dyn Any;
}
dyn_clone::clone_trait_object!(AuthenticationStrategy);

impl Debug for dyn AuthenticationStrategy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let routes = self.get_debug();
        write!(f, "{{{}}}", routes)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BasicAuth {
    pub credentials: String,
}
#[typetag::serde]
impl AuthenticationStrategy for BasicAuth {
    fn check_authentication(&mut self, headers: HeaderMap<HeaderValue>) -> Result<bool, AppError> {
        if headers.is_empty() || !headers.contains_key("Authorization") {
            return Ok(false);
        }
        let value = headers
            .get("Authorization")
            .unwrap()
            .to_str()
            .map_err(|err| AppError(err.to_string()))?;
        let split_list: Vec<_> = value.split(' ').collect();
        if split_list.len() != 2 || split_list[0] != "Basic" {
            return Ok(false);
        }
        let encoded: String = general_purpose::STANDARD_NO_PAD.encode(self.credentials.clone());
        if split_list[1] != encoded {
            return Ok(false);
        }

        Ok(true)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ApiKeyAuth {
    pub key: String,
    pub value: String,
}

#[typetag::serde]
impl AuthenticationStrategy for ApiKeyAuth {
    fn check_authentication(&mut self, headers: HeaderMap<HeaderValue>) -> Result<bool, AppError> {
        if headers.is_empty() || !headers.contains_key(self.key.clone()) {
            return Ok(false);
        }
        let header_value = headers
            .get(self.key.clone())
            .unwrap()
            .to_str()
            .map_err(|err| AppError(err.to_string()))?;
        Ok(header_value == self.value)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
