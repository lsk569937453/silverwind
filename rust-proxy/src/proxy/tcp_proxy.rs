use crate::configuration_service::app_config_service::GLOBAL_CONFIG_MAPPING;
use futures::FutureExt;
use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

pub struct TcpProxy {
    pub listen_port: i32,
    pub mapping_key: String,
    pub channel: mpsc::Receiver<()>,
}
impl TcpProxy {
    pub async fn start_proxy(&mut self) {
        let listen_addr = format!("0.0.0.0:{}", self.listen_port.clone());
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
    route.get_route()
}
