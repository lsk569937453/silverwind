use hyper::body::Incoming;
use tokio::io;

use crate::constants::common_constants::DEFAULT_HTTP_TIMEOUT;
use crate::proxy::http1::http_client::HttpClients;
use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::header::{HeaderValue, CONNECTION, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY, UPGRADE};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use sha1::{Digest, Sha1};
use std::convert::Infallible;
use tokio::io::AsyncWriteExt;

use crate::proxy::proxy_trait::CheckResult;
async fn server_upgraded_io(
    inbound_req: Request<BoxBody<Bytes, Infallible>>,
    outbound_res: Response<Incoming>,
) -> Result<(), anyhow::Error> {
    let upgraded_inbound = hyper::upgrade::on(inbound_req).await?;
    let inbound = TokioIo::new(upgraded_inbound);

    let upgraded_outbound = hyper::upgrade::on(outbound_res).await?;
    let outbound = TokioIo::new(upgraded_outbound);

    let (mut ri, mut wi) = tokio::io::split(inbound);
    let (mut ro, mut wo) = tokio::io::split(outbound);
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
pub async fn server_upgrade(
    req: Request<BoxBody<Bytes, Infallible>>,
    check_result: Option<CheckResult>,
    http_client: HttpClients,
) -> Result<Response<BoxBody<Bytes, Infallible>>, anyhow::Error> {
    debug!("The source request:{:?}.", req);
    let mut res = Response::new(Full::new(Bytes::new()).boxed());
    if !req.headers().contains_key(UPGRADE) {
        *res.status_mut() = StatusCode::BAD_REQUEST;
        return Ok(res);
    }

    let header_map = req.headers().clone();
    let upgrade_value = header_map.get(UPGRADE).unwrap();
    let sec_websocke_key = header_map
        .get(SEC_WEBSOCKET_KEY)
        .ok_or(anyhow!("Can not get the websocket key!"))?
        .to_str()?
        .to_string();

    let request_path = check_result.unwrap().request_path;
    let mut new_request = Request::builder()
        .method(req.method().clone())
        .uri(request_path.clone())
        .body(Full::new(Bytes::new()).boxed())?;

    let new_header = new_request.headers_mut();
    header_map.iter().for_each(|(key, value)| {
        new_header.insert(key, value.clone());
    });
    debug!("The new request is:{:?}", new_request);

    let request_future = if new_request.uri().to_string().contains("https") {
        http_client.request_https(new_request, DEFAULT_HTTP_TIMEOUT)
    } else {
        http_client.request_http(new_request, DEFAULT_HTTP_TIMEOUT)
    };
    let outbound_res = match request_future.await {
        Ok(response) => response.map_err(|e| anyhow!(e.to_string())),
        Err(_) => Err(anyhow!("Request time out,the uri is {}", request_path)),
    }?;
    if outbound_res.status() != StatusCode::SWITCHING_PROTOCOLS {
        return Err(anyhow!("Request error!"));
    }
    tokio::task::spawn(async move {
        let res = server_upgraded_io(req, outbound_res).await;
        if let Err(err) = res {
            error!("{}", err);
        }
    });
    let web_socket_value = format!("{}258EAFA5-E914-47DA-95CA-C5AB0DC85B11", sec_websocke_key);
    let mut hasher = Sha1::new();
    hasher.update(web_socket_value);
    let result = hasher.finalize();
    let encoded: String = general_purpose::STANDARD.encode(result);
    *res.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    res.headers_mut().insert(UPGRADE, upgrade_value.clone());
    res.headers_mut().insert(
        SEC_WEBSOCKET_ACCEPT,
        HeaderValue::from_str(encoded.as_str())?,
    );
    res.headers_mut()
        .insert(CONNECTION, HeaderValue::from_str("Upgrade")?);
    Ok(res)
}
