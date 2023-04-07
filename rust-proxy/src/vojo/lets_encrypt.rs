use std::collections::HashMap;
use std::time::Duration;

use acme_lib::create_p384_key;
use acme_lib::persist::FilePersist;
use acme_lib::{Directory, DirectoryUrl, Error};
use dashmap::DashMap;
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use tokio::time::sleep;
use warp::http::StatusCode;
use warp::Filter;

pub struct LetsEntrypt {
    pub mail_name: String,
    pub domain_name: String,
    pub token_map: Arc<RwLock<DashMap<String, String>>>,
}
impl<'de> Deserialize<'de> for LetsEntrypt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct VistorLetsEntrypt {
            pub mail_name: String,
            pub domain_name: String,
            pub token_map: HashMap<String, String>,
        }

        let vistor_lets_entrypt = VistorLetsEntrypt::deserialize(deserializer)?;
        let dashmap = DashMap::new();
        vistor_lets_entrypt
            .token_map
            .iter()
            .for_each(|(key, value)| {
                dashmap.insert(key.clone(), value.clone());
            });
        let res = LetsEntrypt {
            mail_name: vistor_lets_entrypt.mail_name,
            domain_name: vistor_lets_entrypt.domain_name,
            token_map: Arc::new(RwLock::new(dashmap)),
        };

        Ok(res)
    }
}
pub async fn dyn_reply(
    token: String,
    token_map_shared: Arc<RwLock<DashMap<String, String>>>,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    info!("The server has received the token,the token is {}", token);
    let token_map_lock = token_map_shared.read();
    if let Err(_err) = token_map_lock {
        return Ok(Box::new(StatusCode::BAD_REQUEST));
    }
    let token_map = token_map_lock.unwrap();
    if !token_map.contains_key(&token) {
        error!("Can not find the token:{} from memory.", token);
        return Ok(Box::new(StatusCode::BAD_REQUEST));
    } else {
        // let cloned_map = token_map.clone();
        let proof_option = token_map.get(&token);
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
    token_map: Arc<RwLock<DashMap<String, String>>>,
) -> impl Filter<Extract = (Arc<RwLock<DashMap<String, String>>>,), Error = std::convert::Infallible>
       + Clone {
    warp::any().map(move || token_map.clone())
}
impl LetsEntrypt {
    pub fn _new(mail_name: String, domain_name: String) -> Self {
        LetsEntrypt {
            mail_name,
            domain_name,
            token_map: Arc::new(RwLock::new(DashMap::new())),
        }
    }
    pub async fn start_request(&self) -> Result<(), anyhow::Error> {
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
        tokio::spawn(async move {
            let token_routes = warp::path(".well-known")
                .and(warp::path("acme-challenge"))
                .and(warp::path::param())
                .and(with_token_map(cloned_map))
                .and_then(dyn_reply)
                .with(incoming_log);
            info!("Listening on the port 80");
            let (_addr, server) = warp::serve(token_routes).bind_with_graceful_shutdown(
                ([0, 0, 0, 0], 80),
                async move {
                    rx.recv().await;
                    info!("Close the port 80 successfully!");
                },
            );
            server.await;
        });
        for index in 0..10 {
            info!("start another request,current index is {}", index);
            let request_result = self.request_cert();
            if request_result.is_ok() {
                let send_result = tx.send(()).await.map_err(|e| anyhow!("{}", e));
                return send_result;
            } else {
                error!("{}", request_result.unwrap_err());
            }
            sleep(Duration::from_secs(5)).await;
        }

        Err(anyhow!("Request the lets_encrypt fails"))
    }
    pub fn request_cert(&self) -> Result<(), Error> {
        let url = DirectoryUrl::LetsEncryptStaging;
        let persist = FilePersist::new(".");
        let dir = Directory::from_url(persist, url)?;
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
            let token_lock = self
                .token_map
                .write()
                .map_err(|e| Error::Call(e.to_string()))?;
            token_lock.insert(String::from(token), proof);
            drop(token_lock);
            chall.validate(1000)?;
            ord_new.refresh()?;
        };
        let pkey_pri = create_p384_key();
        let ord_cert = ord_csr.finalize_pkey(pkey_pri, 5000)?;
        let _cert = ord_cert.download_and_save_cert()?;

        Ok(())
    }
}
