//! QUIC transport: one request per stream, with separate
//! notification streaming for pubsub.

use std::sync::Arc;

use log::{debug, error};

use karyon_core::async_util::{select, Either, TaskResult};

use karyon_net::{framed, quic::QuicConn, FramedConn, StreamMux};

use crate::{
    codec::JsonCodec,
    error::Result,
    message,
    server::{
        channel::Channel,
        dispatch::{sanity_check, Handler, NewRequest, SanityCheckResult},
        Server, CHANNEL_SUBSCRIPTION_BUFFER_SIZE,
    },
};

impl Server {
    /// Accept a QUIC connection; each incoming stream is handled
    /// as an independent request.
    pub(super) fn handle_quic_conn(self: &Arc<Self>, quic_conn: QuicConn) -> Result<()> {
        let peer = quic_conn.peer_endpoint().ok();
        debug!("Handle QUIC connection {peer:?}");

        self.task_group.spawn(
            quic_accept_streams_task(self.clone(), Arc::new(quic_conn)),
            move |result: TaskResult<Result<()>>| async move {
                if let TaskResult::Completed(Err(err)) = result {
                    debug!("QUIC conn {peer:?} dropped: {err}");
                } else {
                    debug!("QUIC conn {peer:?} dropped");
                }
            },
        );

        Ok(())
    }
}

async fn quic_accept_streams_task(server: Arc<Server>, quic_conn: Arc<QuicConn>) -> Result<()> {
    let codec = JsonCodec::default();
    loop {
        let stream = quic_conn.accept_stream().await?;
        let conn = framed(stream, codec.clone());
        server.task_group.spawn(
            quic_handle_stream_task(server.clone(), conn),
            |_: TaskResult<Result<()>>| async {},
        );
    }
}

async fn quic_handle_stream_task(server: Arc<Server>, conn: FramedConn<JsonCodec>) -> Result<()> {
    if let Err(err) = handle_quic_stream(server, conn).await {
        error!("Handle QUIC stream: {err}");
    }
    Ok(())
}

/// Handle a single QUIC stream: one request, one response.
/// Upgrades to pubsub notification streaming if the method matches.
async fn handle_quic_stream(server: Arc<Server>, mut conn: FramedConn<JsonCodec>) -> Result<()> {
    let msg = conn.recv_msg().await?;

    let req = match sanity_check(msg) {
        SanityCheckResult::NewReq(req) => req,
        SanityCheckResult::ErrRes(res) => {
            conn.send_msg(serde_json::json!(res)).await?;
            return Ok(());
        }
    };

    // Pubsub handlers need a dedicated stream for streaming notifications.
    // Everything else (regular RPC, method not found) can reuse the
    // shared dispatch path.
    let is_pubsub = matches!(
        server.resolve_handler(&req.srvc_name, &req.method_name, true),
        Handler::Pubsub(_)
    );

    if is_pubsub {
        return handle_quic_subscription(server, conn, req).await;
    }

    let msg = serde_json::to_value(&req.msg).expect("serializable request");
    let response = server.handle_request(None, msg).await;
    debug!("--> {response}");
    conn.send_msg(serde_json::json!(response)).await?;
    Ok(())
}

/// Pubsub over QUIC: split the stream so notifications stream
/// from the writer while the reader waits for an unsubscribe.
async fn handle_quic_subscription(
    server: Arc<Server>,
    conn: FramedConn<JsonCodec>,
    req: NewRequest,
) -> Result<()> {
    let (ch_tx, ch_rx) = async_channel::bounded(CHANNEL_SUBSCRIPTION_BUFFER_SIZE);
    let channel = Channel::new(ch_tx);

    let method = match server.resolve_handler(&req.srvc_name, &req.method_name, true) {
        Handler::Pubsub(m) => m,
        _ => unreachable!("pubsub method presence checked by caller"),
    };

    let params = req.msg.params.unwrap_or(serde_json::json!(()));
    let result = method(channel.clone(), req.msg.method, params).await;

    let response = match result {
        Ok(res) => message::Response {
            result: Some(res),
            id: Some(req.msg.id),
            ..Default::default()
        },
        Err(err) => {
            let mut conn = conn;
            let response = err.to_response(Some(req.msg.id), None);
            conn.send_msg(serde_json::json!(response)).await?;
            return Ok(());
        }
    };

    debug!("--> {response}");

    let (mut reader, mut writer) = conn.split();
    writer.send_msg(serde_json::json!(response)).await?;

    let notification_encoder = server.config.notification_encoder;

    loop {
        match select(ch_rx.recv(), reader.recv_msg()).await {
            Either::Left(nt) => {
                let nt = nt?;
                let notification = notification_encoder(nt);
                debug!("--> {notification}");
                writer.send_msg(serde_json::json!(notification)).await?;
            }
            Either::Right(msg) => match msg {
                Ok(msg) => {
                    let response = server.handle_request(Some(channel.clone()), msg).await;
                    debug!("--> {response}");
                    writer.send_msg(serde_json::json!(response)).await?;
                    channel.close();
                    return Ok(());
                }
                Err(_) => {
                    channel.close();
                    return Ok(());
                }
            },
        }
    }
}
