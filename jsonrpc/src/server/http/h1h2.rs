//! HTTP/1.1 and HTTP/2 over TCP via hyper.

use std::{net::SocketAddr, sync::Arc};

use bytes::Bytes;
use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use hyper::{
    body::{Body, Incoming},
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use hyper_util::server::conn::auto::Builder as HttpConnBuilder;
use log::{debug, error};

use karyon_core::{
    async_runtime::net::{TcpListener, TcpStream},
    async_util::TaskResult,
};

use crate::{
    error::Result,
    message,
    server::{
        http::{
            json_response, json_response_bytes, HyperExecutor, ERR_BODY_TOO_LARGE,
            ERR_METHOD_NOT_ALLOWED, ERR_READ_BODY, MAX_HTTP_BODY_SIZE,
        },
        Server, FAILED_TO_PARSE_ERROR_MSG,
    },
};

pub(super) async fn accept_tcp(server: &Arc<Server>, listener: &TcpListener) {
    match listener.accept().await {
        Ok((stream, peer_addr)) => {
            server.task_group.spawn(
                serve_task(server.clone(), stream, peer_addr),
                |_: TaskResult<Result<()>>| async {},
            );
        }
        Err(err) => {
            error!("Accept TCP connection: {err}");
        }
    }
}

async fn serve_task(server: Arc<Server>, stream: TcpStream, peer_addr: SocketAddr) -> Result<()> {
    if let Err(err) = serve_conn(server, stream, peer_addr).await {
        error!("HTTP/1-2 from {peer_addr}: {err}");
    }
    Ok(())
}

async fn serve_conn(server: Arc<Server>, stream: TcpStream, peer_addr: SocketAddr) -> Result<()> {
    debug!("New HTTP/1-2 connection from {peer_addr}");

    let task_group = server.task_group.clone();
    let service = service_fn(move |req: Request<Incoming>| {
        let server = server.clone();
        async move { handle_hyper_request(server, req).await }
    });

    #[cfg(feature = "smol")]
    let io = smol_hyper::rt::FuturesIo::new(stream);
    #[cfg(feature = "tokio")]
    let io = hyper_util::rt::TokioIo::new(stream);

    let builder = HttpConnBuilder::new(HyperExecutor::new(task_group));
    let conn = builder.serve_connection(io, service);

    if let Err(err) = conn.await {
        debug!("HTTP/1-2 from {peer_addr} closed: {err}");
    }

    Ok(())
}

async fn handle_hyper_request(
    server: Arc<Server>,
    req: Request<Incoming>,
) -> std::result::Result<Response<Full<Bytes>>, hyper::Error> {
    if req.method() != Method::POST {
        return Ok(json_response(
            StatusCode::METHOD_NOT_ALLOWED,
            ERR_METHOD_NOT_ALLOWED,
        ));
    }

    // Fast reject when Content-Length already exceeds the cap.
    if let Some(len) = req.body().size_hint().upper() {
        if len > MAX_HTTP_BODY_SIZE {
            return Ok(json_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                ERR_BODY_TOO_LARGE,
            ));
        }
    }

    // Limited enforces the cap during read regardless of Content-Length.
    let limited = Limited::new(req.into_body(), MAX_HTTP_BODY_SIZE as usize);
    let body = match limited.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(err) => {
            if err.downcast_ref::<LengthLimitError>().is_some() {
                return Ok(json_response(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    ERR_BODY_TOO_LARGE,
                ));
            }
            return Ok(json_response(StatusCode::BAD_REQUEST, ERR_READ_BODY));
        }
    };

    let msg: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            let resp = message::Response {
                error: Some(message::Error {
                    code: message::PARSE_ERROR_CODE,
                    message: FAILED_TO_PARSE_ERROR_MSG.to_string(),
                    data: None,
                }),
                ..Default::default()
            };
            let json = serde_json::to_vec(&resp).unwrap();
            return Ok(json_response_bytes(StatusCode::OK, json));
        }
    };

    let response = server.handle_request(None, msg).await;
    debug!("--> {response}");

    let json = serde_json::to_vec(&response).unwrap();
    Ok(json_response_bytes(StatusCode::OK, json))
}
