pub const DEFAULT_ADMIN_PORT: &'static str = "8870";
pub const DENY_RESPONSE: &'static str = r#"{
    "response_code": -1,
    "response_object": "The request has been blocked by the silverwind!"
}"#;
pub const NOT_FOUND: &'static str = r#"{
    "response_code": -1,
    "response_object": "The route could not be found in the Proxy!"
}"#;
pub const DEFAULT_FIXEDWINDOW_MAP_SIZE: i32 = 3;
pub const ENV_ADMIN_PORT: &'static str = "ADMIN_PORT";
pub const ENV_DATABASE_URL: &'static str = "DATABASE_URL";
pub const ENV_ACCESS_LOG: &'static str = "ACCESS_LOG";
pub const ENV_CONFIG_FILE_PATH: &'static str = "CONFIG_FILE_PATH";
