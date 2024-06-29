mod peer_id;

pub use peer_id::PeerID;

use std::sync::{Arc, Weak};

use async_channel::{Receiver, Sender};
use bincode::{Decode, Encode};
use log::{error, trace};

use karyon_core::{
    async_runtime::{lock::RwLock, Executor},
    async_util::{select, Either, TaskGroup, TaskResult},
    event::{EventListener, EventSys},
    util::{decode, encode},
};

use karyon_net::{Conn, Endpoint};

use crate::{
    conn_queue::ConnDirection,
    message::{NetMsg, NetMsgCmd, ProtocolMsg, ShutdownMsg},
    peer_pool::PeerPool,
    protocol::{Protocol, ProtocolEvent, ProtocolID},
    Config, Error, Result,
};

pub struct Peer {
    /// Peer's ID
    id: PeerID,

    /// A weak pointer to `PeerPool`
    peer_pool: Weak<PeerPool>,

    /// Holds the peer connection
    conn: Conn<NetMsg>,

    /// Remote endpoint for the peer
    remote_endpoint: Endpoint,

    /// The direction of the connection, either `Inbound` or `Outbound`
    conn_direction: ConnDirection,

    /// A list of protocol IDs
    protocol_ids: RwLock<Vec<ProtocolID>>,

    /// `EventSys` responsible for sending events to the protocols.
    protocol_events: Arc<EventSys<ProtocolID>>,

    /// This channel is used to send a stop signal to the read loop.
    stop_chan: (Sender<Result<()>>, Receiver<Result<()>>),

    /// Managing spawned tasks.
    task_group: TaskGroup,
}

impl Peer {
    /// Creates a new peer
    pub fn new(
        peer_pool: Weak<PeerPool>,
        id: &PeerID,
        conn: Conn<NetMsg>,
        remote_endpoint: Endpoint,
        conn_direction: ConnDirection,
        ex: Executor,
    ) -> Arc<Peer> {
        Arc::new(Peer {
            id: id.clone(),
            peer_pool,
            conn,
            protocol_ids: RwLock::new(Vec::new()),
            remote_endpoint,
            conn_direction,
            protocol_events: EventSys::new(),
            task_group: TaskGroup::with_executor(ex),
            stop_chan: async_channel::bounded(1),
        })
    }

    /// Run the peer
    pub async fn run(self: Arc<Self>) -> Result<()> {
        self.start_protocols().await;
        self.read_loop().await
    }

    /// Send a message to the peer connection using the specified protocol.
    pub async fn send<T: Encode + Decode>(&self, protocol_id: &ProtocolID, msg: &T) -> Result<()> {
        let payload = encode(msg)?;

        let proto_msg = ProtocolMsg {
            protocol_id: protocol_id.to_string(),
            payload: payload.to_vec(),
        };

        self.conn
            .send(NetMsg::new(NetMsgCmd::Protocol, &proto_msg)?)
            .await?;
        Ok(())
    }

    /// Broadcast a message to all connected peers using the specified protocol.
    pub async fn broadcast<T: Encode + Decode>(&self, protocol_id: &ProtocolID, msg: &T) {
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
        let shutdown_msg =
            NetMsg::new(NetMsgCmd::Shutdown, ShutdownMsg(0)).expect("pack shutdown message");
        let _ = self.conn.send(shutdown_msg).await;

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
            let fut = select(self.stop_chan.1.recv(), self.conn.recv()).await;
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
    async fn start_protocols(self: &Arc<Self>) {
        for (protocol_id, constructor) in self.peer_pool().protocols.read().await.iter() {
            trace!("peer {} start protocol {protocol_id}", self.id);
            let protocol = constructor(self.clone());

            self.protocol_ids.write().await.push(protocol_id.clone());

            let on_failure = {
                let this = self.clone();
                let protocol_id = protocol_id.clone();
                |result: TaskResult<Result<()>>| async move {
                    if let TaskResult::Completed(res) = result {
                        if res.is_err() {
                            error!("protocol {} stopped", protocol_id);
                        }
                        // Send a stop signal to read loop
                        let _ = this.stop_chan.0.try_send(res);
                    }
                }
            };

            self.task_group.spawn(protocol.start(), on_failure);
        }
    }

    fn peer_pool(&self) -> Arc<PeerPool> {
        self.peer_pool.upgrade().unwrap()
    }
}
