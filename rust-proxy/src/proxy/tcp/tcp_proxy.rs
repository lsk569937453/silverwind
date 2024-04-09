use crate::configuration_service::app_config_service::GLOBAL_CONFIG_MAPPING;
use crate::vojo::app_error::AppError;
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
    pub async fn start_proxy(&mut self) -> Result<(), AppError> {
        let listen_addr = format!("0.0.0.0:{}", self.port.clone());
        let mapping_key_clone = self.mapping_key.clone();
        info!("Listening on: {}", listen_addr);
        let listener = TcpListener::bind(listen_addr)
            .await
            .map_err(|e| AppError(e.to_string()))?;
        let reveiver = &mut self.channel;
        loop {
            let accept_future = listener.accept();
            tokio::select! {
               accept_result=accept_future=>{
                if let Ok((inbound, socket_addr))=accept_result{
                   check(mapping_key_clone.clone(),socket_addr).await?;
                   let transfer = transfer(inbound, mapping_key_clone.clone()).map(|r| {
                        if let Err(e) = r {
                            println!("Failed to transfer,error is {}", e);
                        }
                    });
                    tokio::spawn(transfer);
                }
               },
               _=reveiver.recv()=>{
                info!("close the socket of tcp!");
                return Ok(());
               }
            };
        }
    }
}

async fn transfer(mut inbound: TcpStream, mapping_key: String) -> Result<(), AppError> {
    let proxy_addr = get_route_cluster(mapping_key).await?;
    let mut outbound = TcpStream::connect(proxy_addr)
        .await
        .map_err(|err| AppError(err.to_string()))?;

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
async fn check(mapping_key: String, remote_addr: SocketAddr) -> Result<bool, AppError> {
    let value = GLOBAL_CONFIG_MAPPING
        .get(&mapping_key)
        .ok_or("Can not get apiservice from global_mapping")
        .map_err(|err| AppError(err.to_string()))?;
    let service_config = &value.service_config.routes.clone();
    let service_config_clone = service_config.clone();
    if service_config_clone.is_empty() {
        return Err(AppError(String::from("The len of routes is 0")));
    }
    let route = service_config_clone.first().unwrap();
    let is_allowed = route
        .clone()
        .is_allowed(remote_addr.ip().to_string(), None)
        .await?;
    Ok(is_allowed)
}
async fn get_route_cluster(mapping_key: String) -> Result<String, AppError> {
    let value = GLOBAL_CONFIG_MAPPING
        .get(&mapping_key)
        .ok_or("Can not get apiservice from global_mapping")
        .map_err(|err| AppError(err.to_string()))?;
    let service_config = &value.service_config.routes.clone();
    let service_config_clone = service_config.clone();
    if service_config_clone.is_empty() {
        return Err(AppError(String::from("The len of routes is 0")));
    }
    let mut route = service_config_clone.first().unwrap().route_cluster.clone();
    route.get_route(HeaderMap::new()).await.map(|s| s.endpoint)
}
