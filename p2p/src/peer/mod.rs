mod peer_id;

use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::{Arc, Weak},
};

use async_channel::{Receiver, Sender};
use bincode::Encode;
use log::{debug, error, trace};

use karyon_core::async_util::{select, AsyncQueue, Either, TaskGroup, TaskResult};

#[cfg(feature = "quic")]
use karyon_core::async_runtime::io::{AsyncReadExt, AsyncWriteExt};

use karyon_eventemitter::{EventEmitter, EventListener};

use karyon_net::{FramedReader, FramedWriter};

#[cfg(feature = "quic")]
use karyon_net::{framed, quic::QuicConn, StreamMux};

#[cfg(feature = "quic")]
use crate::message::StreamInit;

use crate::{
    codec::PeerNetMsgCodec,
    conn_queue::QueuedConn,
    endpoint::Endpoint,
    message::{PeerNetCmd, PeerNetMsg, ProtocolMsg, ShutdownMsg},
    peer_pool::PeerPool,
    protocol::{Protocol, ProtocolEvent, ProtocolID},
    util::{decode, encode},
    Config, Error, Result,
};

pub use peer_id::PeerID;

/// Bound on the per-peer outbound queue.
const SEND_QUEUE_SIZE: usize = 128;

/// Direction of a network connection.
#[derive(Clone, Debug)]
pub enum ConnDirection {
    Inbound,
    Outbound,
}

impl fmt::Display for ConnDirection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ConnDirection::Inbound => write!(f, "Inbound"),
            ConnDirection::Outbound => write!(f, "Outbound"),
        }
    }
}

/// A connected peer. Owns the I/O halves end-to-end:
/// the writer is moved into a writer task, the reader into the
/// read loop. Sends go through `send_queue`; the writer task
/// drains it and writes to the wire.
///
/// All immutable-after-construction state is plain (no locks); the
/// remote `id` and `negotiated_protocols` are known by the time the
/// Peer is built.
pub struct Peer {
    own_id: PeerID,
    id: PeerID,
    peer_pool: Weak<PeerPool>,

    direction: ConnDirection,
    remote_endpoint: Endpoint,

    /// Outbound queue. Senders push; the writer task drains.
    send_queue: Arc<AsyncQueue<PeerNetMsg>>,
    /// Disconnect signal sent back to ConnQueue::handle.
    disconnect_signal: Sender<Result<()>>,
    /// Dispatcher for incoming protocol messages.
    protocol_events: Arc<EventEmitter<ProtocolID>>,
    /// Per-protocol listener handles, populated in `from_queued`
    /// before this Peer is wrapped in `Arc`. Read-only after.
    listeners: HashMap<ProtocolID, EventListener<ProtocolID, ProtocolEvent>>,

    /// Per-protocol QUIC send queues, one per stream. Populated in
    /// `from_queued` before this Peer is wrapped in `Arc`. Read-only
    /// after.
    #[cfg(feature = "quic")]
    protocol_send_queues: HashMap<ProtocolID, Arc<AsyncQueue<PeerNetMsg>>>,
    #[cfg(feature = "quic")]
    quic_conn: Option<QuicConn>,

    negotiated_protocols: HashSet<ProtocolID>,
    stop_chan: (Sender<Result<()>>, Receiver<Result<()>>),
    config: Arc<Config>,
    task_group: TaskGroup,
}

impl Peer {
    /// Send a msg using the specified protocol. Routes to the
    /// per-protocol QUIC queue when one exists, otherwise the main
    /// queue.
    pub async fn send<T: Encode>(&self, proto_id: ProtocolID, msg: T) -> Result<()> {
        let payload = encode(&msg)?;
        let proto_msg = ProtocolMsg {
            protocol_id: proto_id.clone(),
            payload: payload.to_vec(),
        };
        let net_msg = PeerNetMsg::new(PeerNetCmd::Protocol, &proto_msg)?;

        #[cfg(feature = "quic")]
        if let Some(q) = self.protocol_send_queues.get(&proto_id) {
            q.push(net_msg).await;
            return Ok(());
        }

        self.send_queue.push(net_msg).await;
        Ok(())
    }

    /// Receive a msg for the specified protocol.
    pub async fn recv<P: Protocol>(&self) -> Result<ProtocolEvent> {
        match self.listeners.get(&P::id()) {
            Some(l) => l.recv().await.map_err(Error::from),
            None => Err(Error::UnsupportedProtocol(P::id())),
        }
    }

    /// Broadcast a message to all connected peers.
    pub async fn broadcast<T: Encode>(&self, proto_id: &ProtocolID, msg: &T) {
        self.peer_pool().broadcast(proto_id, msg).await;
    }

    pub fn id(&self) -> &PeerID {
        &self.id
    }

    pub fn own_id(&self) -> &PeerID {
        &self.own_id
    }

    pub fn config(&self) -> Arc<Config> {
        self.config.clone()
    }

    pub fn remote_endpoint(&self) -> &Endpoint {
        &self.remote_endpoint
    }

    pub fn is_inbound(&self) -> bool {
        matches!(self.direction, ConnDirection::Inbound)
    }

    pub fn direction(&self) -> &ConnDirection {
        &self.direction
    }

    /// Returns the set of protocols negotiated during handshake.
    pub fn negotiated_protocols(&self) -> &HashSet<ProtocolID> {
        &self.negotiated_protocols
    }

    /// Drives the peer end-to-end:
    ///   1. Spawn the main writer task (drains `send_queue`).
    ///   2. Spawn writer + reader tasks for each pre-set-up QUIC stream.
    ///   3. Start negotiated protocols.
    ///   4. Run the read loop on the owned reader.
    pub(crate) async fn run(
        self: Arc<Self>,
        mut reader: FramedReader<PeerNetMsgCodec>,
        writer: FramedWriter<PeerNetMsgCodec>,
        #[cfg(feature = "quic")] quic_streams: Vec<QuicStream>,
    ) -> Result<()> {
        self.spawn_writer_task(writer, self.send_queue.clone());

        #[cfg(feature = "quic")]
        for stream in quic_streams {
            let q = self
                .protocol_send_queues
                .get(&stream.proto_id)
                .expect("queue installed in from_queued")
                .clone();
            self.spawn_writer_task(stream.writer, q);
            self.spawn_quic_reader(stream.proto_id, stream.reader);
        }

        self.run_connect_protocols().await;
        self.read_loop(&mut reader).await
    }

    /// Spawn a task that drains `queue` into `writer`. The writer is
    /// owned by the task; no lock is needed.
    fn spawn_writer_task(
        self: &Arc<Self>,
        mut writer: FramedWriter<PeerNetMsgCodec>,
        queue: Arc<AsyncQueue<PeerNetMsg>>,
    ) {
        self.task_group.spawn(
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

    /// Spawn a reader task for one QUIC protocol stream.
    #[cfg(feature = "quic")]
    fn spawn_quic_reader(
        self: &Arc<Self>,
        proto_id: ProtocolID,
        mut reader: FramedReader<PeerNetMsgCodec>,
    ) {
        let this = self.clone();
        self.task_group.spawn(
            async move {
                loop {
                    let msg = match reader.recv_msg().await {
                        Ok(m) => m,
                        Err(_) => break,
                    };
                    match msg.header.command {
                        PeerNetCmd::Protocol => {
                            let proto_msg: ProtocolMsg = decode(&msg.payload)?.0;
                            this.protocol_events
                                .emit_by_topic(
                                    &proto_msg.protocol_id,
                                    &ProtocolEvent::Message(proto_msg.payload),
                                )
                                .await?;
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

    pub(crate) async fn shutdown(self: &Arc<Self>) -> Result<()> {
        trace!("peer {} shutting down", self.id);

        for proto_id in self.negotiated_protocols.iter() {
            let _ = self
                .protocol_events
                .emit_by_topic(proto_id, &ProtocolEvent::Shutdown)
                .await;
        }

        let _ = self.stop_chan.0.try_send(Ok(()));

        #[cfg(feature = "quic")]
        if let Some(quic_conn) = self.quic_conn.as_ref() {
            quic_conn.close(0, b"shutdown");
        }

        self.disconnect(Ok(())).await?;
        self.task_group.cancel().await;
        Ok(())
    }

    async fn disconnect(&self, res: Result<()>) -> Result<()> {
        self.protocol_events.clear();
        self.disconnect_signal.send(res).await?;
        // Best-effort Shutdown on the wire. The writer task may have
        // already exited; if so the queue is unattended and the
        // message is dropped on task_group cancel.
        let m = PeerNetMsg::new(PeerNetCmd::Shutdown, ShutdownMsg(0))?;
        self.send_queue.push(m).await;
        Ok(())
    }

    /// Start negotiated protocols only.
    async fn run_connect_protocols(self: &Arc<Self>) {
        for (proto_id, constructor) in self.peer_pool().protocols.read().await.iter() {
            if !self.negotiated_protocols.contains(proto_id) {
                trace!("peer {} skip protocol {proto_id} (not negotiated)", self.id);
                continue;
            }
            trace!("peer {} run protocol {proto_id}", self.id);

            let protocol = constructor(self.clone());

            let on_failure = {
                let this = self.clone();
                let proto_id = proto_id.clone();
                |result: TaskResult<Result<()>>| async move {
                    if let TaskResult::Completed(res) = result {
                        if res.is_err() {
                            error!("protocol {proto_id} stopped");
                        }
                        let _ = this.stop_chan.0.try_send(res);
                    }
                }
            };

            self.task_group.spawn(protocol.start(), on_failure);
        }
    }

    /// Read loop: reads messages from the reader half.
    async fn read_loop(&self, reader: &mut FramedReader<PeerNetMsgCodec>) -> Result<()> {
        loop {
            let fut = select(self.stop_chan.1.recv(), reader.recv_msg()).await;
            let result = match fut {
                Either::Left(stop_signal) => {
                    trace!("Peer {} received stop signal", self.id);
                    return stop_signal?;
                }
                Either::Right(result) => result,
            };

            let msg = result?;

            match msg.header.command {
                PeerNetCmd::Protocol => {
                    let msg: ProtocolMsg = decode(&msg.payload)?.0;
                    self.protocol_events
                        .emit_by_topic(&msg.protocol_id, &ProtocolEvent::Message(msg.payload))
                        .await?;
                }
                PeerNetCmd::Shutdown => {
                    return Err(Error::PeerShutdown);
                }
                command => {
                    return Err(Error::InvalidMsg(format!("Unexpected msg {command:?}")));
                }
            }
        }
    }

    fn peer_pool(&self) -> Arc<PeerPool> {
        self.peer_pool.upgrade().unwrap()
    }
}

/// One QUIC protocol stream's halves, returned from `from_queued`
/// for `Peer::run` to spawn tasks against.
#[cfg(feature = "quic")]
pub(crate) struct QuicStream {
    pub proto_id: ProtocolID,
    pub reader: FramedReader<PeerNetMsgCodec>,
    pub writer: FramedWriter<PeerNetMsgCodec>,
}

/// Builder result: pre-Arc state needed to drive `Peer::run`.
pub(crate) struct PeerHalves {
    pub peer: Arc<Peer>,
    pub reader: FramedReader<PeerNetMsgCodec>,
    pub writer: FramedWriter<PeerNetMsgCodec>,
    #[cfg(feature = "quic")]
    pub quic_streams: Vec<QuicStream>,
}

impl Peer {
    /// Build a Peer from the post-handshake `QueuedConn`. All
    /// per-protocol state (listeners, QUIC stream queues) is
    /// populated here before the Peer is wrapped in `Arc`, which is
    /// why no field needs interior mutability.
    pub(crate) async fn from_queued(
        peer_pool: Arc<PeerPool>,
        queued: QueuedConn,
        id: PeerID,
        negotiated_protocols: HashSet<ProtocolID>,
        protocol_ids: impl IntoIterator<Item = ProtocolID>,
    ) -> Result<PeerHalves> {
        let own_id = peer_pool.id.clone();
        let config = peer_pool.config.clone();
        let ex = peer_pool.executor.clone();
        let peer_pool = Arc::downgrade(&peer_pool);
        let send_queue = AsyncQueue::new(SEND_QUEUE_SIZE);
        let protocol_events = EventEmitter::new();

        let mut listeners = HashMap::new();
        for proto_id in protocol_ids {
            let listener = protocol_events.register(&proto_id);
            listeners.insert(proto_id, listener);
        }

        // Open / accept QUIC streams up front so the per-protocol
        // queue map is fully populated before we wrap in Arc.
        #[cfg(feature = "quic")]
        let (protocol_send_queues, quic_streams) =
            setup_quic_streams(&queued, &negotiated_protocols).await?;

        let peer = Arc::new(Peer {
            own_id,
            id,
            peer_pool,
            direction: queued.direction,
            remote_endpoint: queued.remote_endpoint,
            send_queue,
            disconnect_signal: queued.disconnect_signal,
            protocol_events,
            listeners,
            #[cfg(feature = "quic")]
            protocol_send_queues,
            #[cfg(feature = "quic")]
            quic_conn: queued.quic_conn,
            negotiated_protocols,
            config,
            task_group: TaskGroup::with_executor(ex),
            stop_chan: async_channel::bounded(1),
        });

        Ok(PeerHalves {
            peer,
            reader: queued.reader,
            writer: queued.writer,
            #[cfg(feature = "quic")]
            quic_streams,
        })
    }
}

/// Open or accept one stream per negotiated protocol. Returns the
/// (proto_id -> queue) map and the (proto_id, reader, writer)
/// triples for `Peer::run` to wire up.
#[cfg(feature = "quic")]
async fn setup_quic_streams(
    queued: &QueuedConn,
    negotiated: &HashSet<ProtocolID>,
) -> Result<(
    HashMap<ProtocolID, Arc<AsyncQueue<PeerNetMsg>>>,
    Vec<QuicStream>,
)> {
    let mut queues = HashMap::new();
    let mut streams = Vec::new();

    let quic_conn = match queued.quic_conn.as_ref() {
        Some(c) => c,
        None => return Ok((queues, streams)),
    };

    match queued.direction {
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
                streams.push(QuicStream {
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
                streams.push(QuicStream {
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
