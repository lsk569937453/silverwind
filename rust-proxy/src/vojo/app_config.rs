use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Matcher {
    pub prefix: String,
    pub prefix_rewrite: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Route {
    pub matcher: Matcher,
    pub route_cluster: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ApiService {
    pub listen_port: i32,
    pub routes: Vec<Route>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct StaticConifg {
    pub access_log: Option<String>,
    pub database_url: Option<String>,
    pub api_port: String,
    pub config_file_path: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub static_config: StaticConifg,
    pub api_service_config: Vec<ApiService>,
}
mod tests {
    use crate::vojo::app_config;

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
        let route = Route {
            route_cluster: String::from("/"),
            matcher: Matcher {
                prefix: String::from("ss"),
                prefix_rewrite: String::from("ssss"),
            },
        };
        let api_service = ApiService {
            listen_port: 4486,
            routes: vec![route],
        };
        let t = vec![api_service];
        let yaml = serde_yaml::to_string(&t).unwrap();
        println!("{}", yaml);
    }
}
