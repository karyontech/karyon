use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_channel::Sender;
use async_trait::async_trait;
use log::{debug, error};

use karyon_core::async_util::{AsyncQueue, TaskGroup, TaskResult};
use karyon_net::{FramedReader, FramedWriter};

#[cfg(feature = "quic")]
use karyon_core::async_runtime::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(feature = "quic")]
use karyon_net::{framed, quic::QuicConn, StreamMux};

#[cfg(feature = "quic")]
use crate::message::StreamInit;

use crate::{
    codec::PeerNetMsgCodec,
    conn_queue::QueuedConn,
    message::{PeerNetCmd, PeerNetMsg, ProtocolMsg, ShutdownMsg},
    peer::ConnDirection,
    protocol::{ProtocolEvent, ProtocolID},
    util::{decode, encode},
    Error, Result,
};

const SEND_QUEUE_SIZE: usize = 128;
const RECV_QUEUE_SIZE: usize = 128;

/// Per-peer wire abstraction. Hides single-pipe (TCP/TLS) vs.
/// stream-mux (QUIC) framing from the layers above.
#[async_trait]
pub(crate) trait PeerConnection: Send + Sync {
    /// Send pre-encoded payload bytes for `proto_id`.
    async fn send(&self, proto_id: &ProtocolID, payload: Vec<u8>) -> Result<()>;
    /// Pop the next event for `proto_id`. Blocks until a message
    /// arrives or shutdown is broadcast.
    async fn recv(&self, proto_id: &ProtocolID) -> Result<ProtocolEvent>;
    /// Graceful close. Pushes Shutdown to every recv queue and signals
    /// the wire (transport-specific).
    async fn shutdown(&self) -> Result<()>;
}

/// Build the right `PeerConnection` for the post-handshake `QueuedConn`.
/// Picks `MuxConnection` when QUIC is in use, `SingleConnection` otherwise.
pub(crate) async fn from_queued(
    queued: QueuedConn,
    negotiated: &HashSet<ProtocolID>,
    proto_ids: impl IntoIterator<Item = ProtocolID> + Clone,
    task_group: &TaskGroup,
    stop_chan: Sender<Result<()>>,
) -> Result<Arc<dyn PeerConnection>> {
    #[cfg(feature = "quic")]
    if queued.quic_conn.is_some() {
        let conn = MuxConnection::from_queued(queued, negotiated, proto_ids, task_group).await?;
        return Ok(Arc::new(conn) as Arc<dyn PeerConnection>);
    }

    let _ = negotiated;
    let conn = SingleConnection::from_queued(queued, proto_ids, task_group, stop_chan);
    Ok(Arc::new(conn))
}

/// TCP / TLS path: one shared writer drains `send_queue`; a single
/// reader demuxes incoming `PeerNetMsg`s into the matching `recv_queues`.
pub(crate) struct SingleConnection {
    send_queue: Arc<AsyncQueue<PeerNetMsg>>,
    recv_queues: HashMap<ProtocolID, Arc<AsyncQueue<ProtocolEvent>>>,
}

impl SingleConnection {
    /// Spawn the writer + demux reader and return the connection.
    pub(crate) fn from_queued(
        queued: QueuedConn,
        proto_ids: impl IntoIterator<Item = ProtocolID>,
        task_group: &TaskGroup,
        stop_chan: Sender<Result<()>>,
    ) -> Self {
        let send_queue = AsyncQueue::new(SEND_QUEUE_SIZE);
        let recv_queues = build_recv_queues(proto_ids);

        spawn_writer_task(task_group, queued.writer, send_queue.clone());
        spawn_demux_reader(task_group, queued.reader, recv_queues.clone(), stop_chan);

        Self {
            send_queue,
            recv_queues,
        }
    }
}

#[async_trait]
impl PeerConnection for SingleConnection {
    async fn send(&self, proto_id: &ProtocolID, payload: Vec<u8>) -> Result<()> {
        let proto_msg = ProtocolMsg {
            protocol_id: proto_id.clone(),
            payload,
        };
        let net_msg = PeerNetMsg::new(PeerNetCmd::Protocol, &proto_msg)?;
        self.send_queue.push(net_msg).await;
        Ok(())
    }

    async fn recv(&self, proto_id: &ProtocolID) -> Result<ProtocolEvent> {
        match self.recv_queues.get(proto_id) {
            Some(q) => Ok(q.recv().await),
            None => Err(Error::UnsupportedProtocol(proto_id.clone())),
        }
    }

    async fn shutdown(&self) -> Result<()> {
        let m = PeerNetMsg::new(PeerNetCmd::Shutdown, ShutdownMsg(0))?;
        self.send_queue.push(m).await;
        broadcast_shutdown(&self.recv_queues).await;
        Ok(())
    }
}

/// QUIC path: one stream per protocol, each with its own writer task.
/// `send_queues` and `recv_queues` are keyed by protocol id.
#[cfg(feature = "quic")]
pub(crate) struct MuxConnection {
    send_queues: HashMap<ProtocolID, Arc<AsyncQueue<PeerNetMsg>>>,
    recv_queues: HashMap<ProtocolID, Arc<AsyncQueue<ProtocolEvent>>>,
    quic_conn: QuicConn,
}

#[cfg(feature = "quic")]
impl MuxConnection {
    /// Open / accept one QUIC stream per negotiated protocol and spawn
    /// a reader + writer task for each.
    pub(crate) async fn from_queued(
        queued: QueuedConn,
        negotiated: &HashSet<ProtocolID>,
        proto_ids: impl IntoIterator<Item = ProtocolID>,
        task_group: &TaskGroup,
    ) -> Result<Self> {
        let quic_conn = queued
            .quic_conn
            .ok_or_else(|| Error::InvalidMsg("MuxConnection requires a QUIC conn".into()))?;

        let recv_queues = build_recv_queues(proto_ids);
        let (send_queues, streams) =
            setup_quic_streams(&quic_conn, &queued.direction, negotiated).await?;

        for stream in streams {
            let q_send = send_queues
                .get(&stream.proto_id)
                .expect("send queue installed in setup_quic_streams")
                .clone();
            let q_recv = recv_queues
                .get(&stream.proto_id)
                .expect("recv queue installed in build_recv_queues")
                .clone();
            spawn_writer_task(task_group, stream.writer, q_send);
            spawn_quic_reader(task_group, stream.proto_id, stream.reader, q_recv);
        }

        Ok(Self {
            send_queues,
            recv_queues,
            quic_conn,
        })
    }
}

#[cfg(feature = "quic")]
#[async_trait]
impl PeerConnection for MuxConnection {
    async fn send(&self, proto_id: &ProtocolID, payload: Vec<u8>) -> Result<()> {
        let proto_msg = ProtocolMsg {
            protocol_id: proto_id.clone(),
            payload,
        };
        let net_msg = PeerNetMsg::new(PeerNetCmd::Protocol, &proto_msg)?;
        match self.send_queues.get(proto_id) {
            Some(q) => {
                q.push(net_msg).await;
                Ok(())
            }
            None => Err(Error::UnsupportedProtocol(proto_id.clone())),
        }
    }

    async fn recv(&self, proto_id: &ProtocolID) -> Result<ProtocolEvent> {
        match self.recv_queues.get(proto_id) {
            Some(q) => Ok(q.recv().await),
            None => Err(Error::UnsupportedProtocol(proto_id.clone())),
        }
    }

    async fn shutdown(&self) -> Result<()> {
        self.quic_conn.close(0, b"shutdown");
        broadcast_shutdown(&self.recv_queues).await;
        Ok(())
    }
}

/// One QUIC protocol stream's halves, returned from `setup_quic_streams`.
#[cfg(feature = "quic")]
struct MuxStream {
    proto_id: ProtocolID,
    reader: FramedReader<PeerNetMsgCodec>,
    writer: FramedWriter<PeerNetMsgCodec>,
}

/// Spawn a task that drains `queue` into `writer`. Exits when the
/// writer fails (peer hung up).
fn spawn_writer_task(
    task_group: &TaskGroup,
    mut writer: FramedWriter<PeerNetMsgCodec>,
    queue: Arc<AsyncQueue<PeerNetMsg>>,
) {
    task_group.spawn(
        async move {
            loop {
                let msg = queue.recv().await;
                if writer.send_msg(msg).await.is_err() {
                    break;
                }
            }
            Ok::<(), Error>(())
        },
        |res: TaskResult<Result<()>>| async move {
            debug!("Peer writer task ended: {res}");
        },
    );
}

/// Spawn the single-pipe reader task. Reads `PeerNetMsg`s, routes
/// `Protocol` payloads into the matching recv queue, signals the peer
/// via `stop_chan` on Shutdown / error.
fn spawn_demux_reader(
    task_group: &TaskGroup,
    mut reader: FramedReader<PeerNetMsgCodec>,
    recv_queues: HashMap<ProtocolID, Arc<AsyncQueue<ProtocolEvent>>>,
    stop_chan: Sender<Result<()>>,
) {
    task_group.spawn(
        async move {
            loop {
                let msg = match reader.recv_msg().await {
                    Ok(m) => m,
                    Err(e) => {
                        let _ = stop_chan.try_send(Err(e.into()));
                        break;
                    }
                };
                match msg.header.command {
                    PeerNetCmd::Protocol => {
                        let proto_msg: ProtocolMsg = match decode(&msg.payload) {
                            Ok((m, _)) => m,
                            Err(e) => {
                                let _ = stop_chan.try_send(Err(e));
                                break;
                            }
                        };
                        match recv_queues.get(&proto_msg.protocol_id) {
                            Some(q) => {
                                q.push(ProtocolEvent::Message(proto_msg.payload)).await;
                            }
                            None => {
                                error!("No recv queue for protocol {}", proto_msg.protocol_id);
                            }
                        }
                    }
                    PeerNetCmd::Shutdown => {
                        let _ = stop_chan.try_send(Err(Error::PeerShutdown));
                        break;
                    }
                    command => {
                        let _ = stop_chan.try_send(Err(Error::InvalidMsg(format!(
                            "Unexpected msg {command:?}"
                        ))));
                        break;
                    }
                }
            }
            Ok::<(), Error>(())
        },
        |res: TaskResult<Result<()>>| async move {
            debug!("Peer reader task ended: {res}");
        },
    );
}

/// Spawn a per-stream QUIC reader task. The stream is already keyed
/// by protocol id, so messages go straight into `recv_queue`.
#[cfg(feature = "quic")]
fn spawn_quic_reader(
    task_group: &TaskGroup,
    proto_id: ProtocolID,
    mut reader: FramedReader<PeerNetMsgCodec>,
    recv_queue: Arc<AsyncQueue<ProtocolEvent>>,
) {
    task_group.spawn(
        async move {
            loop {
                let msg = match reader.recv_msg().await {
                    Ok(m) => m,
                    Err(_) => break,
                };
                match msg.header.command {
                    PeerNetCmd::Protocol => {
                        let proto_msg: ProtocolMsg = decode(&msg.payload)?.0;
                        recv_queue
                            .push(ProtocolEvent::Message(proto_msg.payload))
                            .await;
                    }
                    PeerNetCmd::Shutdown => break,
                    cmd => {
                        error!("Unexpected msg on QUIC stream {proto_id}: {cmd:?}");
                    }
                }
            }
            Ok::<(), Error>(())
        },
        |res: TaskResult<Result<()>>| async move {
            debug!("QUIC stream reader task ended: {res}");
        },
    );
}

/// Open / accept one QUIC stream per negotiated protocol and return
/// the per-stream send queues plus the reader/writer halves.
#[cfg(feature = "quic")]
async fn setup_quic_streams(
    quic_conn: &QuicConn,
    direction: &ConnDirection,
    negotiated: &HashSet<ProtocolID>,
) -> Result<(
    HashMap<ProtocolID, Arc<AsyncQueue<PeerNetMsg>>>,
    Vec<MuxStream>,
)> {
    let mut queues = HashMap::new();
    let mut streams = Vec::new();

    match direction {
        ConnDirection::Outbound => {
            for proto_id in negotiated.iter() {
                let mut stream = quic_conn.open_stream().await?;

                let init = StreamInit {
                    protocol_id: proto_id.clone(),
                };
                let encoded = encode(&init)?;
                stream.write_all(&encoded).await?;
                stream.flush().await?;

                let conn = framed(stream, PeerNetMsgCodec::new());
                let (reader, writer) = conn.split();

                let q = AsyncQueue::new(SEND_QUEUE_SIZE);
                queues.insert(proto_id.clone(), q);
                streams.push(MuxStream {
                    proto_id: proto_id.clone(),
                    reader,
                    writer,
                });
            }
        }
        ConnDirection::Inbound => {
            let expected = negotiated.len();
            let mut received = 0;

            while received < expected {
                let mut stream = quic_conn.accept_stream().await?;

                let mut header_buf = vec![0u8; 256];
                let n = stream.read(&mut header_buf).await?;
                if n == 0 {
                    continue;
                }

                let (init, _): (StreamInit, _) = decode(&header_buf[..n])?;

                if !negotiated.contains(&init.protocol_id) {
                    error!("Unsupported protocol: {}", init.protocol_id);
                    continue;
                }

                let conn = framed(stream, PeerNetMsgCodec::new());
                let (reader, writer) = conn.split();

                let q = AsyncQueue::new(SEND_QUEUE_SIZE);
                queues.insert(init.protocol_id.clone(), q);
                streams.push(MuxStream {
                    proto_id: init.protocol_id,
                    reader,
                    writer,
                });

                received += 1;
            }
        }
    }

    Ok((queues, streams))
}

/// One bounded recv queue per protocol id.
fn build_recv_queues(
    proto_ids: impl IntoIterator<Item = ProtocolID>,
) -> HashMap<ProtocolID, Arc<AsyncQueue<ProtocolEvent>>> {
    proto_ids
        .into_iter()
        .map(|id| (id, AsyncQueue::new(RECV_QUEUE_SIZE)))
        .collect()
}

/// Push `ProtocolEvent::Shutdown` into every recv queue, waking
/// blocked `recv` callers.
async fn broadcast_shutdown(queues: &HashMap<ProtocolID, Arc<AsyncQueue<ProtocolEvent>>>) {
    for q in queues.values() {
        q.push(ProtocolEvent::Shutdown).await;
    }
}
