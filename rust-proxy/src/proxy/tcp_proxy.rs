use crate::configuration_service::app_config_service::GLOBAL_CONFIG_MAPPING;
use futures::FutureExt;
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
                if let Ok((inbound, _))=accept_result{
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
    let proxy_addr = match get_route_cluster(mapping_key) {
        Ok(r) => r,
        Err(err) => return Err(err),
    };
    let mut outbound = match TcpStream::connect(proxy_addr).await {
        Ok(r) => r,
        Err(err) => return Err(anyhow!(err.to_string())),
    };

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
fn get_route_cluster(mapping_key: String) -> Result<String, anyhow::Error> {
    let option_value = GLOBAL_CONFIG_MAPPING.get(&mapping_key.clone());
    if option_value.is_none() {
        return Err(anyhow!("Can not get apiservice from global_mapping"));
    }
    let service_config = &option_value.unwrap().service_config.routes.clone();
    let service_config_clone = service_config.clone();
    if service_config_clone.len() == 0 {
        return Err(anyhow!("The len of routes is 0"));
    }
    let mut route = service_config_clone.first().unwrap().route_cluster.clone();
    route.get_route().map(|s| s.endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vojo::api_service_manager::ApiServiceManager;
    use crate::vojo::app_config::{Route, ServiceConfig};
    use crate::vojo::route::{BaseRoute, LoadbalancerStrategy, RandomRoute};
    use lazy_static::lazy_static;
    use std::net::TcpListener;
    use std::{thread, time, vec};

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
                weight: 100,
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
                        matcher: Default::default(),
                        route_cluster: route,
                        allow_deny_list: None,
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
}
