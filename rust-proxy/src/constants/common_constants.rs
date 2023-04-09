pub const DEFAULT_ADMIN_PORT: &str = "8870";
pub const DENY_RESPONSE: &str = r#"{
    "response_code": -1,
    "response_object": "The request has been blocked by the silverwind!"
}"#;
pub const NOT_FOUND: &str = r#"{
    "response_code": -1,
    "response_object": "The route could not be found in the Proxy!"
}"#;
pub const DEFAULT_FIXEDWINDOW_MAP_SIZE: i32 = 3;
pub const ENV_ADMIN_PORT: &str = "ADMIN_PORT";
pub const ENV_DATABASE_URL: &str = "DATABASE_URL";
pub const ENV_ACCESS_LOG: &str = "ACCESS_LOG";
pub const ENV_CONFIG_FILE_PATH: &str = "CONFIG_FILE_PATH";
pub const TIMER_WAIT_SECONDS: u64 = 5;
pub const DEFAULT_HTTP_TIMEOUT: u64 = 5;
pub const DEFAULT_TEMPORARY_DIR: &str = "temporary";
