use crate::constants::common_constants::DEFAULT_TEMPORARY_DIR;
use acme_lib::persist::FilePersist;
use acme_lib::{create_p384_key, Certificate};
use acme_lib::{Directory, DirectoryUrl, Error};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::env;
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use warp::http::StatusCode;
use warp::Filter;
use warp::{Rejection, Reply};
#[derive(Debug, Clone, Deserialize, Serialize, Default)]

pub struct LetsEntrypt {
    pub mail_name: String,
    pub domain_name: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub token_map: Arc<DashMap<String, String>>,
}
pub async fn handle_not_found(reject: Rejection) -> Result<impl Reply, Rejection> {
    if reject.is_not_found() {
        Ok(StatusCode::NOT_FOUND)
    } else {
        Err(reject)
    }
}
pub async fn dyn_reply(
    token: String,
    token_map_shared: Arc<DashMap<String, String>>,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    info!("The server has received the token,the token is {}", token);

    if !token_map_shared.contains_key(&token) {
        error!("Can not find the token:{} from memory.", token);
        return Ok(Box::new(StatusCode::BAD_REQUEST));
    } else {
        // let cloned_map = token_map.clone();
        let proof_option = token_map_shared.get(&token);
        if let Some(proof) = proof_option {
            info!(
                "The server response the proof successfully,token:{},proof:{}",
                token,
                proof.clone()
            );
            return Ok(Box::new(proof.clone()));
        }
    }
    Ok(Box::new(StatusCode::BAD_REQUEST))
}
pub fn with_token_map(
    token_map: Arc<DashMap<String, String>>,
) -> impl Filter<Extract = (Arc<DashMap<String, String>>,), Error = std::convert::Infallible> + Clone
{
    warp::any().map(move || token_map.clone())
}
impl LetsEntrypt {
    pub fn _new(mail_name: String, domain_name: String) -> Self {
        LetsEntrypt {
            mail_name,
            domain_name,
            token_map: Arc::new(DashMap::new()),
        }
    }
    pub async fn start_request(&self) -> Result<Certificate, anyhow::Error> {
        let listener = TcpListener::bind("0.0.0.0:80")?;
        drop(listener);

        let incoming_log = warp::log::custom(|info| {
            eprintln!(
                "{} {} {} {:?}",
                info.method(),
                info.path(),
                info.status(),
                info.elapsed(),
            );
        });
        let (tx, mut rx) = mpsc::channel(100);
        let cloned_map = self.token_map.clone();
        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            rt.block_on(async move {
                let token_routes = warp::path(".well-known")
                    .and(warp::path("acme-challenge"))
                    .and(warp::path::param())
                    .and(with_token_map(cloned_map))
                    .and_then(dyn_reply)
                    .with(incoming_log);
                let get_request = warp::get().and(token_routes).recover(handle_not_found);
                info!("Listening on the port 80");
                let (_addr, server) = warp::serve(get_request).bind_with_graceful_shutdown(
                    ([0, 0, 0, 0], 80),
                    async move {
                        rx.recv().await;

                        info!("Close the port 80 successfully!");
                    },
                );
                server.await;
                info!("Stop listening on the port 80");
            });
        });

        let request_result = self.request_cert(DirectoryUrl::LetsEncrypt);
        if request_result.is_ok() {
            let send_result = tx.send(()).await.map_err(|e| anyhow!("{}", e));
            if send_result.is_err() {
                error!(
                    "Close the 80 port error,the error is:{}",
                    send_result.unwrap_err()
                );
            }
            return request_result.map_err(|e| anyhow!("{}", e));
        } else {
            error!("{}", request_result.unwrap_err());
        }

        Err(anyhow!("Request the lets_encrypt fails"))
    }
    pub fn request_cert(&self, directory_url: DirectoryUrl) -> Result<Certificate, Error> {
        let result: bool = Path::new(DEFAULT_TEMPORARY_DIR).is_dir();
        if !result {
            let path = env::current_dir()?;
            let absolute_path = path.join(DEFAULT_TEMPORARY_DIR);
            std::fs::create_dir_all(absolute_path)?;
        }
        let persist = FilePersist::new(DEFAULT_TEMPORARY_DIR);
        let dir = Directory::from_url(persist, directory_url)?;
        let acc = dir.account(&self.mail_name)?;
        let mut ord_new = acc.new_order(&self.domain_name, &[])?;
        let ord_csr = loop {
            if let Some(ord_csr) = ord_new.confirm_validations() {
                break ord_csr;
            }
            let auths = ord_new.authorizations()?;
            let chall = auths[0].http_challenge();
            let token = chall.http_token();
            let proof = chall.http_proof();
            info!("Has receive the token:{} and proof:{}", token, proof);

            self.token_map.insert(String::from(token), proof);
            info!("Has deleted the lock!");

            chall.validate(1000)?;
            ord_new.refresh()?;
        };
        let pkey_pri = create_p384_key();
        let ord_cert = ord_csr.finalize_pkey(pkey_pri, 5000)?;
        let cert = ord_cert.download_and_save_cert()?;

        Ok(cert)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn test_request_cert_ok1() {
        let lets_entrypt = LetsEntrypt::_new(
            String::from("lsk@gmail.com"),
            String::from("www.silverwind.top"),
        );
        let request_result = lets_entrypt.request_cert(DirectoryUrl::LetsEncryptStaging);
        assert!(request_result.is_err());
    }
    #[tokio::test]
    #[ignore]
    async fn test_start_request_ok1() {
        let lets_entrypt = LetsEntrypt::_new(
            String::from("lsk@gmail.com"),
            String::from("www.silverwind.top"),
        );
        let request_result = lets_entrypt.start_request().await;
        assert!(request_result.is_err());
    }
}
