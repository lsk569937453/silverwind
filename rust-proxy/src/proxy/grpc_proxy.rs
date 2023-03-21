use futures::FutureExt;

use h2::client;
use h2::server;
use h2::server::SendResponse;
use h2::RecvStream;
use h2::SendStream;
use http::version::Version;
use http::{Method, Request};
use http::{Response, StatusCode};
use hyper::body::Bytes;
use hyper::service::{make_service_fn, service_fn};
use hyper::HeaderMap;
use hyper::{Client, Error, Server};
use std::convert::TryFrom;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::rustls::{OwnedTrustAnchor, RootCertStore, ServerName};
use tokio_rustls::TlsConnector;
pub struct GrpcProxy {
    // pub port: i32,
    // pub channel: mpsc::Receiver<()>,
    // pub mapping_key: String,
}

pub async fn start_main() -> Result<(), anyhow::Error> {
    let mut listener = TcpListener::bind("127.0.0.1:5928").await.unwrap();

    // Accept all incoming TCP connections.
    loop {
        if let Ok((socket, _peer_addr)) = listener.accept().await {
            // Spawn a new task to process each connection.
            tokio::spawn(async {
                // Start the HTTP/2.0 connection handshake
                let mut h2 = server::handshake(socket).await.unwrap();
                // Accept all inbound HTTP/2.0 streams sent over the
                // connection.
                while let Some(request) = h2.accept().await {
                    let (request, mut respond) = request.unwrap();
                    println!("Received request: {:?}", request);

                    request_outbound(request, respond).await.unwrap();
                    // Build a response with no body
                    // let response = Response::builder()
                    //     .header("content-type", "application/grpc")
                    //     .status(StatusCode::OK)
                    //     .body(())
                    //     .unwrap();

                    // // Send the response back to the client
                    // let mut s = respond.send_response(response, false).unwrap();
                    // let data = Bytes::from_static(b"\0\0\0\0\x1a\n\x18Hello enim dolore veniam");
                    // s.send_data(data, false).unwrap();
                    // let mut header = HeaderMap::new();
                    // header.insert("content-type", "application/grpc".parse().unwrap());

                    // s.send_trailers(header).unwrap();
                }
            });
        }
    }
}
async fn request_outbound(
    mut inbount_request: Request<RecvStream>,
    mut inbound_respond: SendResponse<Bytes>,
) -> Result<(), anyhow::Error> {
    let tcp = TcpStream::connect("127.0.0.1:50051")
        .await
        .map_err(|e| anyhow!(e.to_string()))?;
    let (h2, connection) = client::handshake(tcp)
        .await
        .map_err(|e| anyhow!(e.to_string()))?;
    tokio::spawn(async move {
        connection.await.unwrap();
    });

    let mut h2 = h2.ready().await.map_err(|e| anyhow!(e.to_string()))?;
    // Prepare the HTTP request to send to the server.
    let request = Request::builder()
        .method(Method::POST)
        .uri("http://localhost:50051/helloworld.Greeter/SayHello")
        .header("content-type", "application/grpc")
        .body(())
        .unwrap();

    // Send the request. The second tuple item allows the caller
    // to stream a request body.
    let (response, mut outbound_sendStream) = h2.send_request(request, false).unwrap();
    let body = inbount_request.body_mut();

    let mut count = 0;
    while let Some(data) = body.data().await {
        let data = data.map_err(|e| anyhow!(e.to_string()))?;
        println!("<<<< recv {:?}", data);
        let _ = body.flow_control().release_capacity(data.len());
        outbound_sendStream
            .send_data(data.clone(), count == 1)
            .unwrap();
        count = count + 1;
    }

    let (head, mut outboud_response_body) = response
        .await
        .map_err(|e| anyhow!(e.to_string()))?
        .into_parts();

    println!("Received response: {:?}", head);
    let inbound_response = Response::builder()
        .header("content-type", "application/grpc")
        .status(StatusCode::OK)
        .version(Version::HTTP_2)
        .body(())
        .unwrap();

    let mut send_stream = inbound_respond
        .send_response(inbound_response, false)
        .map_err(|e| anyhow!(e.to_string()))?;
    // The `flow_control` handle allows the caller to manage
    // flow control.
    //
    // Whenever data is received, the caller is responsible for
    // releasing capacity back to the server once it has freed
    // the data from memory.
    let mut flow_control = outboud_response_body.flow_control().clone();

    // let body = inbount_request.body_mut();

    while let Some(chunk) = outboud_response_body.data().await {
        let chunk_bytes = chunk.map_err(|e| anyhow!(e.to_string()))?;
        println!("RX: {:?}", chunk_bytes.clone());

        let trtr = send_stream.send_data(chunk_bytes.clone(), false).unwrap();
        // Let the server send more data.
        let _ = flow_control.release_capacity(chunk_bytes.len()).unwrap();
    }
    let mut headers = HeaderMap::new();
    headers.insert("grpc-status", "0".parse().unwrap());
    headers.insert("trace-proto-bin", "jher831yy13JHy3hc".parse().unwrap());
    send_stream.send_trailers(headers).unwrap();
    println!("ss");
    Ok(())
}
