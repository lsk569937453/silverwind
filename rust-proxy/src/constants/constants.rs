pub const DEFAULT_API_PORT: &'static str = "8870";
pub const DENY_RESPONSE: &'static str = r#"{
    "response_code": -1,
    "response_object": "The request has been blocked by the silverwind!"
}"#;
pub const NOT_FOUND: &'static str = r#"{
    "response_code": -1,
    "response_object": "The route could not be found in the Proxy!"
}"#;
pub const DEFAULT_FIXEDWINDOW_MAP_SIZE: i32 = 3;
