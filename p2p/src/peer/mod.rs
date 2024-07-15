mod peer_id;

use std::sync::{Arc, Weak};

use async_channel::{Receiver, Sender};
use bincode::Encode;
use log::{error, trace};
use parking_lot::RwLock;

use karyon_core::{
    async_runtime::Executor,
    async_util::{select, Either, TaskGroup, TaskResult},
    util::decode,
};

use crate::{
    connection::{ConnDirection, Connection},
    endpoint::Endpoint,
    message::{NetMsgCmd, ProtocolMsg},
    peer_pool::PeerPool,
    protocol::{InitProtocol, Protocol, ProtocolEvent, ProtocolID},
    protocols::HandshakeProtocol,
    Config, Error, Result,
};

pub use peer_id::PeerID;

pub struct Peer {
    /// Own ID
    own_id: PeerID,

    /// Peer's ID
    id: RwLock<Option<PeerID>>,

    /// A weak pointer to [`PeerPool`]
    peer_pool: Weak<PeerPool>,

    /// Holds the peer connection
    pub(crate) conn: Arc<Connection>,

    /// This channel is used to send a stop signal to the read loop.
    stop_chan: (Sender<Result<()>>, Receiver<Result<()>>),

    /// The Configuration for the P2P network.
    config: Arc<Config>,

    /// Managing spawned tasks.
    task_group: TaskGroup,
}

impl Peer {
    /// Creates a new peer
    pub(crate) fn new(
        own_id: PeerID,
        peer_pool: Weak<PeerPool>,
        conn: Arc<Connection>,
        config: Arc<Config>,
        ex: Executor,
    ) -> Arc<Peer> {
        Arc::new(Peer {
            own_id,
            id: RwLock::new(None),
            peer_pool,
            conn,
            config,
            task_group: TaskGroup::with_executor(ex),
            stop_chan: async_channel::bounded(1),
        })
    }

    /// Send a msg to this peer connection using the specified protocol.
    pub async fn send<T: Encode>(&self, proto_id: ProtocolID, msg: T) -> Result<()> {
        self.conn.send(proto_id, msg).await
    }

    /// Receives a new msg from this peer connection.
    pub async fn recv<P: Protocol>(&self) -> Result<ProtocolEvent> {
        self.conn.recv::<P>().await
    }

    /// Broadcast a message to all connected peers using the specified protocol.
    pub async fn broadcast<T: Encode>(&self, proto_id: &ProtocolID, msg: &T) {
        self.peer_pool().broadcast(proto_id, msg).await;
    }

    /// Returns the peer's ID
    pub fn id(&self) -> Option<PeerID> {
        self.id.read().clone()
    }

    /// Returns own ID
    pub fn own_id(&self) -> &PeerID {
        &self.own_id
    }

    /// Returns the [`Config`]
    pub fn config(&self) -> Arc<Config> {
        self.config.clone()
    }

    /// Returns the remote endpoint for the peer
    pub fn remote_endpoint(&self) -> &Endpoint {
        &self.conn.remote_endpoint
    }

    /// Check if the connection is Inbound
    pub fn is_inbound(&self) -> bool {
        match self.conn.direction {
            ConnDirection::Inbound => true,
            ConnDirection::Outbound => false,
        }
    }

    /// Returns the direction of the connection, which can be either `Inbound`
    /// or `Outbound`.
    pub fn direction(&self) -> &ConnDirection {
        &self.conn.direction
    }

    pub(crate) async fn init(self: &Arc<Self>) -> Result<()> {
        let handshake_protocol = HandshakeProtocol::new(
            self.clone(),
            self.peer_pool().protocol_versions.read().await.clone(),
        );

        let pid = handshake_protocol.init().await?;
        *self.id.write() = Some(pid);

        Ok(())
    }

    /// Run the peer
    pub(crate) async fn run(self: Arc<Self>) -> Result<()> {
        self.run_connect_protocols().await;
        self.read_loop().await
    }

    /// Shuts down the peer
    pub(crate) async fn shutdown(self: &Arc<Self>) -> Result<()> {
        trace!("peer {:?} shutting down", self.id());

        // Send shutdown event to the attached protocols
        for proto_id in self.peer_pool().protocols.read().await.keys() {
            let _ = self.conn.emit_msg(proto_id, &ProtocolEvent::Shutdown).await;
        }

        // Send a stop signal to the read loop
        //
        // No need to handle the error here; a dropped channel and
        // sendig a stop signal have the same effect.
        let _ = self.stop_chan.0.try_send(Ok(()));

        self.conn.disconnect(Ok(())).await?;

        // Force shutting down
        self.task_group.cancel().await;
        Ok(())
    }

    /// Run running the Connect Protocols for this peer connection.
    async fn run_connect_protocols(self: &Arc<Self>) {
        for (proto_id, constructor) in self.peer_pool().protocols.read().await.iter() {
            trace!("peer {:?} run protocol {proto_id}", self.id());

            let protocol = constructor(self.clone());

            let on_failure = {
                let this = self.clone();
                let proto_id = proto_id.clone();
                |result: TaskResult<Result<()>>| async move {
                    if let TaskResult::Completed(res) = result {
                        if res.is_err() {
                            error!("protocol {} stopped", proto_id);
                        }
                        // Send a stop signal to read loop
                        let _ = this.stop_chan.0.try_send(res);
                    }
                }
            };

            self.task_group.spawn(protocol.start(), on_failure);
        }
    }

    /// Run a read loop to handle incoming messages from the peer connection.
    async fn read_loop(&self) -> Result<()> {
        loop {
            let fut = select(self.stop_chan.1.recv(), self.conn.recv_inner()).await;
            let result = match fut {
                Either::Left(stop_signal) => {
                    trace!("Peer {:?} received a stop signal", self.id());
                    return stop_signal?;
                }
                Either::Right(result) => result,
            };

            let msg = result?;

            match msg.header.command {
                NetMsgCmd::Protocol => {
                    let msg: ProtocolMsg = decode(&msg.payload)?.0;
                    self.conn
                        .emit_msg(&msg.protocol_id, &ProtocolEvent::Message(msg.payload))
                        .await?;
                }
                NetMsgCmd::Shutdown => {
                    return Err(Error::PeerShutdown);
                }
                command => return Err(Error::InvalidMsg(format!("Unexpected msg {:?}", command))),
            }
        }
    }

    /// Returns `PeerPool` pointer
    fn peer_pool(&self) -> Arc<PeerPool> {
        self.peer_pool.upgrade().unwrap()
    }
}
