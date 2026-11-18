//! QUIC transport: one request per stream, with separate
//! notification streaming for pubsub.

use std::sync::Arc;

use log::{debug, error};

use karyon_core::async_util::{Either, TaskResult, select};

use karyon_net::{
    Endpoint, FramedConn, StreamMux, framed,
    quic::{QuicConn, QuicIncoming},
};

use crate::{
    codec::JsonCodec,
    error::Result,
    message,
    server::{
        CHANNEL_SUBSCRIPTION_BUFFER_SIZE, Server,
        channel::Channel,
        dispatch::{Handler, SanityCheckResult, run_handler, sanity_check},
    },
};

impl Server {
    /// Run the handshake off the accept loop, then serve the connection.
    pub(super) fn handle_quic_incoming(self: &Arc<Self>, incoming: QuicIncoming) {
        let peer = incoming.peer_endpoint();
        let server = self.clone();
        self.task_group.spawn(async move {
            // Bounded by the QUIC idle timeout (QuicConfig::idle_timeout).
            match incoming.handshake().await {
                Ok(quic_conn) => server.handle_quic_conn(quic_conn),
                Err(err) => debug!("QUIC handshake with {peer} failed: {err}"),
            }
        });
    }

    /// Serve a QUIC connection; each incoming stream is handled
    /// as an independent request.
    fn handle_quic_conn(self: &Arc<Self>, quic_conn: QuicConn) {
        let peer = quic_conn.peer_endpoint().ok();
        debug!("Handle QUIC connection {peer:?}");

        self.task_group.spawn_then(
            quic_accept_streams_task(self.clone(), Arc::new(quic_conn)),
            move |result: TaskResult<Result<()>>| async move {
                if let TaskResult::Completed(Err(err)) = result {
                    debug!("QUIC conn {peer:?} dropped: {err}");
                } else {
                    debug!("QUIC conn {peer:?} dropped");
                }
            },
        );
    }
}

async fn quic_accept_streams_task(server: Arc<Server>, quic_conn: Arc<QuicConn>) -> Result<()> {
    let codec = JsonCodec::default();
    let peer = quic_conn.peer_endpoint().ok();
    loop {
        let stream = quic_conn.accept_stream().await?;
        let conn = framed(stream, codec.clone());
        server
            .task_group
            .spawn(quic_handle_stream_task(server.clone(), conn, peer.clone()));
    }
}

async fn quic_handle_stream_task(
    server: Arc<Server>,
    conn: FramedConn<JsonCodec>,
    peer: Option<Endpoint>,
) -> Result<()> {
    if let Err(err) = handle_quic_stream(server, conn, peer).await {
        error!("Handle QUIC stream: {err}");
    }
    Ok(())
}

/// Handle a single QUIC stream: one request, one response.
/// Upgrades to pubsub notification streaming if the method matches.
async fn handle_quic_stream(
    server: Arc<Server>,
    mut conn: FramedConn<JsonCodec>,
    peer: Option<Endpoint>,
) -> Result<()> {
    let msg = conn.recv_msg().await?;

    let req = match sanity_check(msg) {
        SanityCheckResult::NewReq(req) => req,
        SanityCheckResult::ErrRes(res) => {
            conn.send_msg(serde_json::json!(res)).await?;
            return Ok(());
        }
    };

    // Pubsub handlers need a dedicated stream for streaming notifications.
    let handler = server.resolve_handler(&req.srvc_name, &req.method_name, true);
    if let Handler::Pubsub(_) = handler {
        return handle_quic_subscription(&server, conn, handler, req.msg, peer).await;
    }

    let response = run_handler(handler, req.msg, None).await;
    debug!("--> {response}");
    conn.send_msg(serde_json::json!(response)).await?;
    Ok(())
}

/// Pubsub over QUIC: split the stream so notifications stream
/// from the writer while the reader waits for an unsubscribe.
async fn handle_quic_subscription(
    server: &Server,
    mut conn: FramedConn<JsonCodec>,
    handler: Handler<'_>,
    req: message::Request,
    peer: Option<Endpoint>,
) -> Result<()> {
    let (ch_tx, ch_rx) = async_channel::bounded(CHANNEL_SUBSCRIPTION_BUFFER_SIZE);
    let channel = Channel::new(ch_tx, peer);

    let response = run_handler(handler, req, Some(channel.clone())).await;
    if response.error.is_some() {
        conn.send_msg(serde_json::json!(response)).await?;
        return Ok(());
    }

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
