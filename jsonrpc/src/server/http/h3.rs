//! HTTP/3 over QUIC via h3 / h3-quinn.

use std::{collections::HashMap, sync::Arc};

use bytes::{Buf, Bytes};
use h3_quinn::Connection;
use hyper::{Method, Request, Response, StatusCode};
use log::{debug, error};

use karyon_core::{async_runtime::lock::RwLock, async_util::TaskResult};

use karyon_net::quic::{QuicConn, QuicEndpoint};

use crate::{
    error::{Error, Result},
    message::{self, SubscriptionID},
    server::{
        channel::{Channel, NewNotification},
        http::{ERR_BODY_TOO_LARGE, ERR_METHOD_NOT_ALLOWED, MAX_HTTP_BODY_SIZE},
        Server, CHANNEL_SUBSCRIPTION_BUFFER_SIZE, FAILED_TO_PARSE_ERROR_MSG,
    },
};

type H3Stream = h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>;

/// Map from `SubscriptionID` to the channel sender that feeds that
/// subscription's HTTP/3 reply stream. Used by `dispatch_subs_task`
/// to look up the right sender for each incoming `NewNotification`
/// and forward the notification to its stream.
///
/// Inserted on subscribe, removed on unsubscribe (or when the stream
/// ends). `RwLock` because the dispatcher reads it on every
/// notification (frequent), while subscribe / unsubscribe write
/// rarely.
type SubSenders = Arc<RwLock<HashMap<SubscriptionID, async_channel::Sender<NewNotification>>>>;

pub(super) async fn accept_h3(server: &Arc<Server>, quic_ep: &QuicEndpoint) {
    let quic_conn = match quic_ep.accept().await {
        Ok(conn) => conn,
        Err(err) => {
            error!("Accept QUIC for HTTP/3: {err}");
            return;
        }
    };

    server.task_group.spawn(
        serve_conn_task(server.clone(), quic_conn),
        |_: TaskResult<Result<()>>| async {},
    );
}

async fn serve_conn_task(server: Arc<Server>, quic_conn: QuicConn) -> Result<()> {
    let peer = quic_conn
        .peer_endpoint()
        .map(|e| e.to_string())
        .unwrap_or_default();
    if let Err(err) = serve_conn(server, quic_conn).await {
        debug!("HTTP/3 from {peer} closed: {err}");
    }
    Ok(())
}

async fn serve_conn(server: Arc<Server>, quic_conn: QuicConn) -> Result<()> {
    let h3_conn = Connection::new(quic_conn.inner().clone());
    let mut h3_server: h3::server::Connection<Connection, Bytes> =
        h3::server::Connection::new(h3_conn)
            .await
            .map_err(|e| Error::HttpError(format!("H3 handshake: {e}")))?;

    // Per-connection channel feeds all pubsub output.
    let (ch_tx, ch_rx) = async_channel::bounded(CHANNEL_SUBSCRIPTION_BUFFER_SIZE);
    let channel = Channel::new(ch_tx);

    let sub_senders: SubSenders = Arc::new(RwLock::new(HashMap::new()));

    // Dispatcher task: route notifications from the connection-wide
    // channel to the right per-subscription sender.
    server.task_group.spawn(
        dispatch_subs_task(ch_rx, sub_senders.clone()),
        |_: TaskResult<Result<()>>| async {},
    );

    loop {
        match h3_server.accept().await {
            Ok(Some(resolver)) => {
                let (req, stream) = match resolver.resolve_request().await {
                    Ok(v) => v,
                    Err(err) => {
                        error!("Resolve HTTP/3 request: {err}");
                        continue;
                    }
                };
                server.task_group.spawn(
                    handle_request_task(
                        server.clone(),
                        channel.clone(),
                        sub_senders.clone(),
                        req,
                        stream,
                    ),
                    |_: TaskResult<Result<()>>| async {},
                );
            }
            Ok(None) => break,
            Err(err) => {
                debug!("HTTP/3 connection error: {err}");
                break;
            }
        }
    }

    for (_, sender) in sub_senders.write().await.drain() {
        sender.close();
    }
    channel.close();
    Ok(())
}

async fn dispatch_subs_task(
    ch_rx: async_channel::Receiver<NewNotification>,
    sub_senders: SubSenders,
) -> Result<()> {
    while let Ok(nt) = ch_rx.recv().await {
        let subs = sub_senders.read().await;
        if let Some(sender) = subs.get(&nt.sub_id) {
            let _ = sender.send(nt).await;
        }
    }
    Ok(())
}

async fn handle_request_task(
    server: Arc<Server>,
    channel: Arc<Channel>,
    sub_senders: SubSenders,
    req: Request<()>,
    stream: H3Stream,
) -> Result<()> {
    if let Err(err) = handle_h3_request(server, channel, sub_senders, req, stream).await {
        error!("Handle HTTP/3 request: {err}");
    }
    Ok(())
}

async fn handle_h3_request(
    server: Arc<Server>,
    channel: Arc<Channel>,
    sub_senders: SubSenders,
    req: Request<()>,
    mut stream: H3Stream,
) -> Result<()> {
    if req.method() != Method::POST {
        h3_send(
            &mut stream,
            StatusCode::METHOD_NOT_ALLOWED,
            ERR_METHOD_NOT_ALLOWED.as_bytes(),
        )
        .await?;
        return Ok(());
    }

    let mut body = Vec::new();
    while let Some(chunk) = stream.recv_data().await.map_err(h3_err)? {
        body.extend_from_slice(Buf::chunk(&chunk));
        if body.len() as u64 > MAX_HTTP_BODY_SIZE {
            h3_send(
                &mut stream,
                StatusCode::PAYLOAD_TOO_LARGE,
                ERR_BODY_TOO_LARGE.as_bytes(),
            )
            .await?;
            return Ok(());
        }
    }

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
            h3_send(
                &mut stream,
                StatusCode::OK,
                &serde_json::to_vec(&resp).unwrap(),
            )
            .await?;
            return Ok(());
        }
    };

    let is_subscribe = msg
        .get("method")
        .and_then(|m| m.as_str())
        .map(|m| m.ends_with("_subscribe") && !m.ends_with("_unsubscribe"))
        .unwrap_or(false);

    if is_subscribe {
        return handle_h3_subscribe(server, sub_senders, msg, stream).await;
    }

    let response = server.handle_request(Some(channel.clone()), msg).await;
    debug!("--> {response}");

    if response.error.is_none() {
        if let Ok(rpc_req) = serde_json::from_slice::<message::Request>(&body) {
            if let Some(params) = &rpc_req.params {
                if let Ok(sub_id) = serde_json::from_value::<SubscriptionID>(params.clone()) {
                    if let Some(sender) = sub_senders.write().await.remove(&sub_id) {
                        sender.close();
                    }
                }
            }
        }
    }

    h3_send(
        &mut stream,
        StatusCode::OK,
        &serde_json::to_vec(&response).unwrap(),
    )
    .await?;
    Ok(())
}

/// Subscribe: respond, then stream notifications on the same stream.
async fn handle_h3_subscribe(
    server: Arc<Server>,
    sub_senders: SubSenders,
    msg: serde_json::Value,
    mut stream: H3Stream,
) -> Result<()> {
    let (sub_tx, sub_rx) = async_channel::bounded(CHANNEL_SUBSCRIPTION_BUFFER_SIZE);
    let sub_channel = Channel::new(sub_tx.clone());

    let response = server.handle_request(Some(sub_channel.clone()), msg).await;

    if response.error.is_some() {
        h3_send(
            &mut stream,
            StatusCode::OK,
            &serde_json::to_vec(&response).unwrap(),
        )
        .await?;
        return Ok(());
    }

    let sub_id = response
        .result
        .as_ref()
        .and_then(|v| serde_json::from_value::<SubscriptionID>(v.clone()).ok())
        .ok_or_else(|| Error::InvalidMsg("Missing subscription id".into()))?;

    sub_senders.write().await.insert(sub_id, sub_tx);

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(())
        .unwrap();
    stream.send_response(resp).await.map_err(h3_err)?;

    debug!("--> {response}");
    let json = serde_json::to_vec(&response).unwrap();
    stream.send_data(Bytes::from(json)).await.map_err(h3_err)?;

    let encoder = server.config.notification_encoder;

    while let Ok(nt) = sub_rx.recv().await {
        let notification = encoder(nt);
        debug!("--> {notification}");
        let json = serde_json::to_vec(&serde_json::json!(notification)).unwrap();
        if stream.send_data(Bytes::from(json)).await.is_err() {
            break;
        }
    }

    sub_senders.write().await.remove(&sub_id);
    sub_channel.close();
    let _ = stream.finish().await;
    Ok(())
}

async fn h3_send(stream: &mut H3Stream, status: StatusCode, body: &[u8]) -> Result<()> {
    let resp = Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(())
        .map_err(|e| Error::HttpError(e.to_string()))?;
    stream.send_response(resp).await.map_err(h3_err)?;
    stream
        .send_data(Bytes::from(body.to_vec()))
        .await
        .map_err(h3_err)?;
    stream.finish().await.map_err(h3_err)?;
    Ok(())
}

fn h3_err(e: impl std::fmt::Display) -> Error {
    Error::HttpError(e.to_string())
}
