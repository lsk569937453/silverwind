use crate::configuration_service::app_config_service::GLOBAL_CONFIG_MAPPING;
use futures::FutureExt;
use http::HeaderMap;
use std::net::SocketAddr;
use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
pub struct TcpProxy {
    pub port: i32,
    pub mapping_key: String,
    pub channel: mpsc::Receiver<()>,
}
impl TcpProxy {
    pub async fn start_proxy(&mut self) {
        let listen_addr = format!("0.0.0.0:{}", self.port.clone());
        let mapping_key_clone = self.mapping_key.clone();
        info!("Listening on: {}", listen_addr);
        let listener = TcpListener::bind(listen_addr).await.unwrap();
        let reveiver = &mut self.channel;
        loop {
            let accept_future = listener.accept();
            tokio::select! {
               accept_result=accept_future=>{
                if let Ok((inbound, socket_addr))=accept_result{
                   let check_result= check(mapping_key_clone.clone(),socket_addr);
                   if check_result.is_err(){
                    return ;
                   }
                   let transfer = transfer(inbound, mapping_key_clone.clone()).map(|r| {
                        if let Err(e) = r {
                            println!("Failed to transfer; error={}", e);
                        }
                    });
                    tokio::spawn(transfer);
                }
               },
               _=reveiver.recv()=>{
                info!("close the socket!");
                return ;
               }
            };
        }
    }
}

async fn transfer(mut inbound: TcpStream, mapping_key: String) -> Result<(), anyhow::Error> {
    let proxy_addr = get_route_cluster(mapping_key)?;
    let mut outbound = TcpStream::connect(proxy_addr)
        .await
        .map_err(|err| anyhow!(err.to_string()))?;

    let (mut ri, mut wi) = inbound.split();
    let (mut ro, mut wo) = outbound.split();
    let client_to_server = async {
        io::copy(&mut ri, &mut wo).await?;
        wo.shutdown().await
    };

    let server_to_client = async {
        io::copy(&mut ro, &mut wi).await?;
        wi.shutdown().await
    };

    let result = tokio::try_join!(client_to_server, server_to_client);

    if result.is_err() {
        error!("Copy stream error!");
    }

    Ok(())
}
fn check(mapping_key: String, remote_addr: SocketAddr) -> Result<bool, anyhow::Error> {
    let value = GLOBAL_CONFIG_MAPPING
        .get(&mapping_key.clone())
        .ok_or("Can not get apiservice from global_mapping")
        .map_err(|err| anyhow!(err.to_string()))?;
    let service_config = &value.service_config.routes.clone();
    let service_config_clone = service_config.clone();
    if service_config_clone.len() == 0 {
        return Err(anyhow!("The len of routes is 0"));
    }
    let route = service_config_clone.first().unwrap();
    let is_allowed = route
        .clone()
        .is_allowed(remote_addr.ip().to_string(), None)?;
    Ok(is_allowed)
}
fn get_route_cluster(mapping_key: String) -> Result<String, anyhow::Error> {
    let value = GLOBAL_CONFIG_MAPPING
        .get(&mapping_key.clone())
        .ok_or("Can not get apiservice from global_mapping")
        .map_err(|err| anyhow!(err.to_string()))?;
    let service_config = &value.service_config.routes.clone();
    let service_config_clone = service_config.clone();
    if service_config_clone.len() == 0 {
        return Err(anyhow!("The len of routes is 0"));
    }
    let mut route = service_config_clone.first().unwrap().route_cluster.clone();
    route.get_route(HeaderMap::new()).map(|s| s.endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration_service::app_config_service::GLOBAL_APP_CONFIG;
    use crate::vojo::allow_deny_ip::AllowDenyObject;
    use crate::vojo::allow_deny_ip::AllowType;
    use crate::vojo::api_service_manager::ApiServiceManager;
    use crate::vojo::app_config::get_route_id;
    use crate::vojo::app_config::{Route, ServiceConfig};
    use crate::vojo::route::{BaseRoute, LoadbalancerStrategy, RandomRoute};
    use lazy_static::lazy_static;
    use std::net::TcpListener;
    use std::net::{IpAddr, Ipv4Addr};
    use std::{thread, time, vec};

    use crate::vojo::app_config::ApiService;
    use crate::vojo::app_config::Matcher;

    use tokio::runtime::{Builder, Runtime};

    lazy_static! {
        pub static ref TOKIO_RUNTIME: Runtime = Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("my-custom-name")
            .thread_stack_size(3 * 1024 * 1024)
            .max_blocking_threads(1000)
            .enable_all()
            .build()
            .unwrap();
    }
    #[test]
    fn test_start_proxy_ok() {
        TOKIO_RUNTIME.spawn(async {
            let (_, receiver) = tokio::sync::mpsc::channel(10);

            let mut tcp_proxy = TcpProxy {
                port: 3352,
                channel: receiver,
                mapping_key: String::from("random key"),
            };
            tcp_proxy.start_proxy().await;
        });
        TOKIO_RUNTIME.spawn(async {
            let listener = TcpListener::bind("127.0.0.1:3352");
            assert_eq!(listener.is_err(), true);
        });
        let sleep_time = time::Duration::from_millis(200);
        thread::sleep(sleep_time);
    }
    #[test]
    fn test_transfer_error() {
        TOKIO_RUNTIME.spawn(async {
            let tcp_stream = TcpStream::connect("httpbin.org:80").await.unwrap();
            let result = transfer(tcp_stream, String::from("test")).await;
            assert_eq!(result.is_err(), true);
        });
        let sleep_time = time::Duration::from_millis(2000);
        thread::sleep(sleep_time);
    }
    #[test]
    fn test_transfer_ok() {
        let route = Box::new(RandomRoute {
            routes: vec![BaseRoute {
                endpoint: String::from("httpbin.org:80"),
                try_file: None,
            }],
        }) as Box<dyn LoadbalancerStrategy>;
        TOKIO_RUNTIME.spawn(async {
            let (sender, _) = tokio::sync::mpsc::channel(10);

            let api_service_manager = ApiServiceManager {
                sender: sender,
                service_config: ServiceConfig {
                    key_str: None,
                    server_type: crate::vojo::app_config::ServiceType::TCP,
                    cert_str: None,
                    routes: vec![Route {
                        route_id: get_route_id(),
                        matcher: Default::default(),
                        route_cluster: route,
                        allow_deny_list: None,
                        authentication: None,
                        ratelimit: None,
                    }],
                },
            };
            GLOBAL_CONFIG_MAPPING.insert(String::from("test123"), api_service_manager);
            let tcp_stream = TcpStream::connect("httpbin.org:80").await.unwrap();
            let result = transfer(tcp_stream, String::from("test123")).await;
            assert_eq!(result.is_ok(), true);
        });
        let sleep_time = time::Duration::from_millis(2000);
        thread::sleep(sleep_time);
    }
    #[test]
    fn test_get_route_cluster_error() {
        let result = get_route_cluster(String::from("testxxxx"));
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_check_deny_all() {
        TOKIO_RUNTIME.block_on(async {
            let route = Box::new(RandomRoute {
                routes: vec![BaseRoute {
                    endpoint: String::from("httpbin.org:80"),
                    try_file: None,
                }],
            }) as Box<dyn LoadbalancerStrategy>;
            let (sender, _) = tokio::sync::mpsc::channel(10);

            let api_service_manager = ApiServiceManager {
                sender: sender,
                service_config: ServiceConfig {
                    key_str: None,
                    server_type: crate::vojo::app_config::ServiceType::TCP,
                    cert_str: None,
                    routes: vec![Route {
                        route_id: get_route_id(),
                        matcher: Some(Matcher {
                            prefix: String::from("/"),
                            prefix_rewrite: String::from("test"),
                        }),
                        route_cluster: route,
                        allow_deny_list: Some(vec![AllowDenyObject {
                            limit_type: AllowType::DENYWALL,
                            value: None,
                        }]),
                        authentication: None,
                        ratelimit: None,
                    }],
                },
            };
            let mut write = GLOBAL_APP_CONFIG.write().await;
            write.api_service_config.push(ApiService {
                listen_port: 3478,
                service_config: api_service_manager.service_config.clone(),
            });
            GLOBAL_CONFIG_MAPPING.insert(String::from("3478-TCP"), api_service_manager);
            let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
            let res = check(String::from("3478-TCP"), socket);
            assert_eq!(res.is_ok(), true);
            assert_eq!(res.unwrap(), false);
        });
    }
    #[test]
    fn test_check_deny_ip() {
        TOKIO_RUNTIME.block_on(async {
            let route = Box::new(RandomRoute {
                routes: vec![BaseRoute {
                    endpoint: String::from("httpbin.org:80"),
                    try_file: None,
                }],
            }) as Box<dyn LoadbalancerStrategy>;
            let (sender, _) = tokio::sync::mpsc::channel(10);

            let api_service_manager = ApiServiceManager {
                sender: sender,
                service_config: ServiceConfig {
                    key_str: None,
                    server_type: crate::vojo::app_config::ServiceType::TCP,
                    cert_str: None,
                    routes: vec![Route {
                        route_id: get_route_id(),
                        matcher: Some(Matcher {
                            prefix: String::from("/"),
                            prefix_rewrite: String::from("test"),
                        }),
                        route_cluster: route,
                        allow_deny_list: Some(vec![AllowDenyObject {
                            limit_type: AllowType::DENY,
                            value: Some(String::from("127.0.0.1")),
                        }]),
                        authentication: None,
                        ratelimit: None,
                    }],
                },
            };
            let mut write = GLOBAL_APP_CONFIG.write().await;
            write.api_service_config.push(ApiService {
                listen_port: 3479,
                service_config: api_service_manager.service_config.clone(),
            });
            GLOBAL_CONFIG_MAPPING.insert(String::from("3479-TCP"), api_service_manager);
            let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
            let res = check(String::from("3479-TCP"), socket);
            assert_eq!(res.is_ok(), true);
            assert_eq!(res.unwrap(), false);
        });
    }
}
