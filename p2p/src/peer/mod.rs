mod peer_id;

pub use peer_id::PeerID;

use std::sync::Arc;

use log::{error, trace};
use smol::{
    channel::{self, Receiver, Sender},
    lock::RwLock,
};

use karyons_core::{
    async_utils::{select, Either, TaskGroup, TaskResult},
    event::{ArcEventSys, EventListener, EventSys},
    utils::{decode, encode},
    Executor,
};

use karyons_net::Endpoint;

use crate::{
    connection::ConnDirection,
    io_codec::{CodecMsg, IOCodec},
    message::{NetMsgCmd, ProtocolMsg, ShutdownMsg},
    peer_pool::{ArcPeerPool, WeakPeerPool},
    protocol::{Protocol, ProtocolEvent, ProtocolID},
    Config, Error, Result,
};

pub type ArcPeer = Arc<Peer>;

pub struct Peer {
    /// Peer's ID
    id: PeerID,

    /// A weak pointer to `PeerPool`
    peer_pool: WeakPeerPool,

    /// Holds the IOCodec for the peer connection
    io_codec: IOCodec,

    /// Remote endpoint for the peer
    remote_endpoint: Endpoint,

    /// The direction of the connection, either `Inbound` or `Outbound`
    conn_direction: ConnDirection,

    /// A list of protocol IDs
    protocol_ids: RwLock<Vec<ProtocolID>>,

    /// `EventSys` responsible for sending events to the protocols.
    protocol_events: ArcEventSys<ProtocolID>,

    /// This channel is used to send a stop signal to the read loop.
    stop_chan: (Sender<Result<()>>, Receiver<Result<()>>),

    /// Managing spawned tasks.
    task_group: TaskGroup,
}

impl Peer {
    /// Creates a new peer
    pub fn new(
        peer_pool: WeakPeerPool,
        id: &PeerID,
        io_codec: IOCodec,
        remote_endpoint: Endpoint,
        conn_direction: ConnDirection,
    ) -> ArcPeer {
        Arc::new(Peer {
            id: id.clone(),
            peer_pool,
            io_codec,
            protocol_ids: RwLock::new(Vec::new()),
            remote_endpoint,
            conn_direction,
            protocol_events: EventSys::new(),
            task_group: TaskGroup::new(),
            stop_chan: channel::bounded(1),
        })
    }

    /// Run the peer
    pub async fn run(self: Arc<Self>, ex: Executor<'_>) -> Result<()> {
        self.start_protocols(ex.clone()).await;
        self.read_loop().await
    }

    /// Send a message to the peer connection using the specified protocol.
    pub async fn send<T: CodecMsg>(&self, protocol_id: &ProtocolID, msg: &T) -> Result<()> {
        let payload = encode(msg)?;

        let proto_msg = ProtocolMsg {
            protocol_id: protocol_id.to_string(),
            payload: payload.to_vec(),
        };

        self.io_codec.write(NetMsgCmd::Protocol, &proto_msg).await?;
        Ok(())
    }

    /// Broadcast a message to all connected peers using the specified protocol.
    pub async fn broadcast<T: CodecMsg>(&self, protocol_id: &ProtocolID, msg: &T) {
        self.peer_pool().broadcast(protocol_id, msg).await;
    }

    /// Shuts down the peer
    pub async fn shutdown(&self) {
        trace!("peer {} start shutting down", self.id);

        // Send shutdown event to all protocols
        for protocol_id in self.protocol_ids.read().await.iter() {
            self.protocol_events
                .emit_by_topic(protocol_id, &ProtocolEvent::Shutdown)
                .await;
        }

        // Send a stop signal to the read loop
        //
        // No need to handle the error here; a dropped channel and
        // sending a stop signal have the same effect.
        let _ = self.stop_chan.0.try_send(Ok(()));

        // No need to handle the error here
        let _ = self
            .io_codec
            .write(NetMsgCmd::Shutdown, &ShutdownMsg(0))
            .await;

        // Force shutting down
        self.task_group.cancel().await;
    }

    /// Check if the connection is Inbound
    #[inline]
    pub fn is_inbound(&self) -> bool {
        match self.conn_direction {
            ConnDirection::Inbound => true,
            ConnDirection::Outbound => false,
        }
    }

    /// Returns the direction of the connection, which can be either `Inbound`
    /// or `Outbound`.
    #[inline]
    pub fn direction(&self) -> &ConnDirection {
        &self.conn_direction
    }

    /// Returns the remote endpoint for the peer
    #[inline]
    pub fn remote_endpoint(&self) -> &Endpoint {
        &self.remote_endpoint
    }

    /// Return the peer's ID
    #[inline]
    pub fn id(&self) -> &PeerID {
        &self.id
    }

    /// Returns the `Config` instance.
    pub fn config(&self) -> Arc<Config> {
        self.peer_pool().config.clone()
    }

    /// Registers a listener for the given Protocol `P`.
    pub async fn register_listener<P: Protocol>(&self) -> EventListener<ProtocolID, ProtocolEvent> {
        self.protocol_events.register(&P::id()).await
    }

    /// Start a read loop to handle incoming messages from the peer connection.
    async fn read_loop(&self) -> Result<()> {
        loop {
            let fut = select(self.stop_chan.1.recv(), self.io_codec.read()).await;
            let result = match fut {
                Either::Left(stop_signal) => {
                    trace!("Peer {} received a stop signal", self.id);
                    return stop_signal?;
                }
                Either::Right(result) => result,
            };

            let msg = result?;

            match msg.header.command {
                NetMsgCmd::Protocol => {
                    let msg: ProtocolMsg = decode(&msg.payload)?.0;

                    if !self.protocol_ids.read().await.contains(&msg.protocol_id) {
                        return Err(Error::UnsupportedProtocol(msg.protocol_id));
                    }

                    let proto_id = &msg.protocol_id;
                    let msg = ProtocolEvent::Message(msg.payload);
                    self.protocol_events.emit_by_topic(proto_id, &msg).await;
                }
                NetMsgCmd::Shutdown => {
                    return Err(Error::PeerShutdown);
                }
                command => return Err(Error::InvalidMsg(format!("Unexpected msg {:?}", command))),
            }
        }
    }

    /// Start running the protocols for this peer connection.
    async fn start_protocols(self: &Arc<Self>, ex: Executor<'_>) {
        for (protocol_id, constructor) in self.peer_pool().protocols.read().await.iter() {
            trace!("peer {} start protocol {protocol_id}", self.id);
            let protocol = constructor(self.clone());

            self.protocol_ids.write().await.push(protocol_id.clone());

            let selfc = self.clone();
            let exc = ex.clone();
            let proto_idc = protocol_id.clone();

            let on_failure = |result: TaskResult<Result<()>>| async move {
                if let TaskResult::Completed(res) = result {
                    if res.is_err() {
                        error!("protocol {} stopped", proto_idc);
                    }
                    // Send a stop signal to read loop
                    let _ = selfc.stop_chan.0.try_send(res);
                }
            };

            self.task_group
                .spawn(ex.clone(), protocol.start(exc), on_failure);
        }
    }

    fn peer_pool(&self) -> ArcPeerPool {
        self.peer_pool.upgrade().unwrap()
    }
}
