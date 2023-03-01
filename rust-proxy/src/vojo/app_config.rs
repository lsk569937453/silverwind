use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize,Default)]
pub struct Matcher {
    prefix: String,
    prefix_rewrite: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize,Default)]
pub struct Route {
    matcher: Matcher,
    route_cluster: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize,Default)]
pub struct ApiService {
    listen_port: i32,
    routes: Vec<Route>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize,Default)]
pub struct AppConfig {
    access_log: String,
    api_service: ApiService,
}
mod tests {
    use super::*;
    // fn run_test<T>(test: T) -> ()
    // where
    //     T: FnOnce() -> () + panic::UnwindSafe,
    // {
    //     setup();
    //     let result = panic::catch_unwind(|| test());
    //     teardown();
    //     assert!(result.is_ok())
    // }
    #[test]
    fn test_output_serde() {
        let app_config:AppConfig = AppConfig { access_log: String::from("sss"), api_service: Default::default() };
        let yaml = serde_yaml::to_string(&app_config).unwrap();
    }
}
