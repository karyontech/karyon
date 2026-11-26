//! QUIC transport: one request per stream, with separate
//! notification streaming for pubsub.

use std::sync::Arc;

use async_trait::async_trait;
use log::{debug, error};

use karyon_core::async_util::{Either, TaskResult, select};

use karyon_net::{
    Endpoint, FramedConn, StreamMux, framed,
    quic::{QuicConn, QuicConnRef, QuicEndpoint, QuicIncoming},
};

use crate::{
    codec::JsonRpcCodec,
    error::{Error, Result},
    message,
    server::{
        CHANNEL_SUBSCRIPTION_BUFFER_SIZE, Server, ServerRef,
        channel::Channel,
        dispatch::{Handler, SanityCheckResult, run_handler, sanity_check},
    },
};

/// Runs a QUIC server. Hides the codec type, like `HttpAcceptor` does
/// for HTTP.
#[async_trait]
pub(crate) trait QuicAcceptor: Send + Sync {
    /// Run the accept loop.
    async fn accept_loop(&self, server: Arc<Server>) -> Result<()>;
    fn local_endpoint(&self) -> Result<Endpoint>;
}

/// `Arc` reference to a `QuicAcceptor`.
pub(crate) type QuicAcceptorRef = Arc<dyn QuicAcceptor>;

/// QUIC server. Every stream is framed with `codec`.
pub(crate) struct QuicServer<C> {
    pub(super) endpoint: QuicEndpoint,
    pub(super) codec: C,
}

#[async_trait]
impl<C: JsonRpcCodec> QuicAcceptor for QuicServer<C> {
    async fn accept_loop(&self, server: Arc<Server>) -> Result<()> {
        // Back off on accept errors, so the loop does not spin.
        let backoff = server.accept_backoff();
        loop {
            match self.endpoint.accept_incoming().await {
                Ok(incoming) => {
                    backoff.reset();
                    handle_quic_incoming(&server, incoming, self.codec.clone());
                }
                Err(err) => {
                    error!("Accept QUIC conn: {err}");
                    backoff.sleep().await;
                }
            }
        }
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        self.endpoint.local_endpoint().map_err(Error::from)
    }
}

/// Run the handshake off the accept loop, then serve the connection.
fn handle_quic_incoming<C: JsonRpcCodec>(server: &ServerRef, incoming: QuicIncoming, codec: C) {
    let peer = incoming.peer_endpoint();
    let srv = server.clone();
    server.task_group.spawn(async move {
        // Bounded by the QUIC idle timeout (QuicConfig::idle_timeout).
        match incoming.handshake().await {
            Ok(quic_conn) => handle_quic_conn(&srv, quic_conn, codec),
            Err(err) => debug!("QUIC handshake with {peer} failed: {err}"),
        }
    });
}

/// Serve a QUIC connection; each incoming stream is handled
/// as an independent request.
fn handle_quic_conn<C: JsonRpcCodec>(server: &ServerRef, quic_conn: QuicConn, codec: C) {
    let peer = quic_conn.peer_endpoint().ok();
    debug!("Handle QUIC connection {peer:?}");

    server.task_group.spawn_then(
        quic_accept_streams_task(server.clone(), Arc::new(quic_conn), codec),
        move |result: TaskResult<Result<()>>| async move {
            if let TaskResult::Completed(Err(err)) = result {
                debug!("QUIC conn {peer:?} dropped: {err}");
            } else {
                debug!("QUIC conn {peer:?} dropped");
            }
        },
    );
}

async fn quic_accept_streams_task<C: JsonRpcCodec>(
    server: ServerRef,
    quic_conn: QuicConnRef,
    codec: C,
) -> Result<()> {
    let peer = quic_conn.peer_endpoint().ok();
    loop {
        let stream = quic_conn.accept_stream().await?;
        let conn = framed(stream, codec.clone());
        server
            .task_group
            .spawn(quic_handle_stream_task(server.clone(), conn, peer.clone()));
    }
}

async fn quic_handle_stream_task<C: JsonRpcCodec>(
    server: ServerRef,
    conn: FramedConn<C>,
    peer: Option<Endpoint>,
) -> Result<()> {
    if let Err(err) = handle_quic_stream(server, conn, peer).await {
        error!("Handle QUIC stream: {err}");
    }
    Ok(())
}

/// Handle a single QUIC stream: one request, one response.
/// Upgrades to pubsub notification streaming if the method matches.
async fn handle_quic_stream<C: JsonRpcCodec>(
    server: ServerRef,
    mut conn: FramedConn<C>,
    peer: Option<Endpoint>,
) -> Result<()> {
    // A stream that never sends its request is dropped after
    // `read_timeout`. Only this first read: after it, a pubsub stream
    // waits for its unsubscribe, which is idle by design.
    let msg = match server.config.read_timeout {
        Some(t) => {
            server
                .task_group
                .executor()
                .timeout(t, conn.recv_msg())
                .await??
        }
        None => conn.recv_msg().await?,
    };

    let req = match sanity_check(&server, msg) {
        SanityCheckResult::NewReq(req) => req,
        SanityCheckResult::ErrRes(res) => {
            conn.send_msg(serde_json::json!(res)).await?;
            return Ok(());
        }
    };

    // Pubsub handlers need a dedicated stream for streaming notifications.
    let handler = server.resolve_handler(&req, true);
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
async fn handle_quic_subscription<C: JsonRpcCodec>(
    server: &Server,
    mut conn: FramedConn<C>,
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
