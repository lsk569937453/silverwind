use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Matcher {
    prefix: String,
    prefix_rewrite: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Route {
    matcher: Matcher,
    route_cluster: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiService {
    listen_port: i32,
    routes: Vec<Route>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        let app_config = AppConfig {
            access_log: String::new("s"),
            api_service: Default(),
        };
    }
}
