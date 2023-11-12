use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::Duration,
};

use log::{error, info, trace, warn};
use smol::{
    channel::Sender,
    lock::{Mutex, RwLock},
};

use karyons_core::{
    async_utils::{TaskGroup, TaskResult},
    utils::decode,
    Executor,
};

use karyons_net::Conn;

use crate::{
    config::Config,
    connection::{ConnDirection, ConnQueue},
    io_codec::{CodecMsg, IOCodec},
    message::{get_msg_payload, NetMsg, NetMsgCmd, VerAckMsg, VerMsg},
    monitor::{Monitor, PeerPoolEvent},
    peer::{ArcPeer, Peer, PeerID},
    protocol::{Protocol, ProtocolConstructor, ProtocolID},
    protocols::PingProtocol,
    utils::{version_match, Version, VersionInt},
    Error, Result,
};

pub type ArcPeerPool = Arc<PeerPool>;
pub type WeakPeerPool = Weak<PeerPool>;

pub struct PeerPool {
    /// Peer's ID
    pub id: PeerID,

    /// Connection queue
    conn_queue: Arc<ConnQueue>,

    /// Holds the running peers.
    peers: Mutex<HashMap<PeerID, ArcPeer>>,

    /// Hashmap contains protocol constructors.
    pub(crate) protocols: RwLock<HashMap<ProtocolID, Box<ProtocolConstructor>>>,

    /// Hashmap contains protocol IDs and their versions.
    protocol_versions: Arc<RwLock<HashMap<ProtocolID, Version>>>,

    /// Managing spawned tasks.
    task_group: TaskGroup,

    /// The Configuration for the P2P network.
    pub config: Arc<Config>,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,
}

impl PeerPool {
    /// Creates a new PeerPool
    pub fn new(
        id: &PeerID,
        conn_queue: Arc<ConnQueue>,
        config: Arc<Config>,
        monitor: Arc<Monitor>,
    ) -> Arc<Self> {
        let protocols = RwLock::new(HashMap::new());
        let protocol_versions = Arc::new(RwLock::new(HashMap::new()));

        Arc::new(Self {
            id: id.clone(),
            conn_queue,
            peers: Mutex::new(HashMap::new()),
            protocols,
            protocol_versions,
            task_group: TaskGroup::new(),
            monitor,
            config,
        })
    }

    /// Start
    pub async fn start(self: &Arc<Self>, ex: Executor<'_>) -> Result<()> {
        self.setup_protocols().await?;
        let selfc = self.clone();
        self.task_group
            .spawn(ex.clone(), selfc.listen_loop(ex.clone()), |_| async {});
        Ok(())
    }

    /// Listens to a new connection from the connection queue
    pub async fn listen_loop(self: Arc<Self>, ex: Executor<'_>) {
        loop {
            let new_conn = self.conn_queue.next().await;
            let disconnect_signal = new_conn.disconnect_signal;

            let result = self
                .new_peer(
                    new_conn.conn,
                    &new_conn.direction,
                    disconnect_signal.clone(),
                    ex.clone(),
                )
                .await;

            if result.is_err() {
                let _ = disconnect_signal.send(()).await;
            }
        }
    }

    /// Shuts down
    pub async fn shutdown(&self) {
        for (_, peer) in self.peers.lock().await.iter() {
            peer.shutdown().await;
        }

        self.task_group.cancel().await;
    }

    /// Attach a custom protocol to the network
    pub async fn attach_protocol<P: Protocol>(&self, c: Box<ProtocolConstructor>) -> Result<()> {
        let protocol_versions = &mut self.protocol_versions.write().await;
        let protocols = &mut self.protocols.write().await;

        protocol_versions.insert(P::id(), P::version()?);
        protocols.insert(P::id(), Box::new(c) as Box<ProtocolConstructor>);
        Ok(())
    }

    /// Returns the number of currently connected peers.
    pub async fn peers_len(&self) -> usize {
        self.peers.lock().await.len()
    }

    /// Broadcast a message to all connected peers using the specified protocol.
    pub async fn broadcast<T: CodecMsg>(&self, proto_id: &ProtocolID, msg: &T) {
        for (pid, peer) in self.peers.lock().await.iter() {
            if let Err(err) = peer.send(proto_id, msg).await {
                error!("failed to send msg to {pid}: {err}");
                continue;
            }
        }
    }

    /// Add a new peer to the peer list.
    pub async fn new_peer(
        self: &Arc<Self>,
        conn: Conn,
        conn_direction: &ConnDirection,
        disconnect_signal: Sender<()>,
        ex: Executor<'_>,
    ) -> Result<PeerID> {
        let endpoint = conn.peer_endpoint()?;
        let io_codec = IOCodec::new(conn);

        // Do a handshake with a connection before creating a new peer.
        let pid = self.do_handshake(&io_codec, conn_direction).await?;

        // TODO: Consider restricting the subnet for inbound connections
        if self.contains_peer(&pid).await {
            return Err(Error::PeerAlreadyConnected);
        }

        // Create a new peer
        let peer = Peer::new(
            Arc::downgrade(self),
            &pid,
            io_codec,
            endpoint.clone(),
            conn_direction.clone(),
        );

        // Insert the new peer
        self.peers.lock().await.insert(pid.clone(), peer.clone());

        let selfc = self.clone();
        let pid_c = pid.clone();
        let on_disconnect = |result| async move {
            if let TaskResult::Completed(_) = result {
                if let Err(err) = selfc.remove_peer(&pid_c).await {
                    error!("Failed to remove peer {pid_c}: {err}");
                }
                let _ = disconnect_signal.send(()).await;
            }
        };

        self.task_group
            .spawn(ex.clone(), peer.run(ex.clone()), on_disconnect);

        info!("Add new peer {pid}, direction: {conn_direction}, endpoint: {endpoint}");

        self.monitor
            .notify(&PeerPoolEvent::NewPeer(pid.clone()).into())
            .await;
        Ok(pid)
    }

    /// Checks if the peer list contains a peer with the given peer id
    pub async fn contains_peer(&self, pid: &PeerID) -> bool {
        self.peers.lock().await.contains_key(pid)
    }

    /// Shuts down the peer and remove it from the peer list.
    async fn remove_peer(&self, pid: &PeerID) -> Result<()> {
        let mut peers = self.peers.lock().await;
        let result = peers.remove(pid);

        drop(peers);

        let peer = match result {
            Some(p) => p,
            None => return Ok(()),
        };

        peer.shutdown().await;

        self.monitor
            .notify(&PeerPoolEvent::RemovePeer(pid.clone()).into())
            .await;

        let endpoint = peer.remote_endpoint();
        let direction = peer.direction();

        warn!("Peer {pid} removed, direction: {direction}, endpoint: {endpoint}",);
        Ok(())
    }

    /// Attach the core protocols.
    async fn setup_protocols(&self) -> Result<()> {
        self.attach_protocol::<PingProtocol>(Box::new(PingProtocol::new))
            .await
    }

    /// Initiate a handshake with a connection.
    async fn do_handshake(
        &self,
        io_codec: &IOCodec,
        conn_direction: &ConnDirection,
    ) -> Result<PeerID> {
        match conn_direction {
            ConnDirection::Inbound => {
                let pid = self.wait_vermsg(io_codec).await?;
                self.send_verack(io_codec).await?;
                Ok(pid)
            }
            ConnDirection::Outbound => {
                self.send_vermsg(io_codec).await?;
                self.wait_verack(io_codec).await
            }
        }
    }

    /// Send a Version message
    async fn send_vermsg(&self, io_codec: &IOCodec) -> Result<()> {
        let pids = self.protocol_versions.read().await;
        let protocols = pids.iter().map(|p| (p.0.clone(), p.1.v.clone())).collect();
        drop(pids);

        let vermsg = VerMsg {
            peer_id: self.id.clone(),
            protocols,
            version: self.config.version.v.clone(),
        };

        trace!("Send VerMsg");
        io_codec.write(NetMsgCmd::Version, &vermsg).await?;
        Ok(())
    }

    /// Wait for a Version message
    ///
    /// Returns the peer's ID upon successfully receiving the Version message.
    async fn wait_vermsg(&self, io_codec: &IOCodec) -> Result<PeerID> {
        let timeout = Duration::from_secs(self.config.handshake_timeout);
        let msg: NetMsg = io_codec.read_timeout(timeout).await?;

        let payload = get_msg_payload!(Version, msg);
        let (vermsg, _) = decode::<VerMsg>(&payload)?;

        if !version_match(&self.config.version.req, &vermsg.version) {
            return Err(Error::IncompatibleVersion("system: {}".into()));
        }

        self.protocols_match(&vermsg.protocols).await?;

        trace!("Received VerMsg from: {}", vermsg.peer_id);
        Ok(vermsg.peer_id)
    }

    /// Send a Verack message
    async fn send_verack(&self, io_codec: &IOCodec) -> Result<()> {
        let verack = VerAckMsg(self.id.clone());

        trace!("Send VerAckMsg");
        io_codec.write(NetMsgCmd::Verack, &verack).await?;
        Ok(())
    }

    /// Wait for a Verack message
    ///
    /// Returns the peer's ID upon successfully receiving the Verack message.
    async fn wait_verack(&self, io_codec: &IOCodec) -> Result<PeerID> {
        let timeout = Duration::from_secs(self.config.handshake_timeout);
        let msg: NetMsg = io_codec.read_timeout(timeout).await?;

        let payload = get_msg_payload!(Verack, msg);
        let (verack, _) = decode::<VerAckMsg>(&payload)?;

        trace!("Received VerAckMsg from: {}", verack.0);
        Ok(verack.0)
    }

    /// Check if the new connection has compatible protocols.
    async fn protocols_match(&self, protocols: &HashMap<ProtocolID, VersionInt>) -> Result<()> {
        for (n, pv) in protocols.iter() {
            let pids = self.protocol_versions.read().await;

            match pids.get(n) {
                Some(v) => {
                    if !version_match(&v.req, pv) {
                        return Err(Error::IncompatibleVersion(format!("{n} protocol: {pv}")));
                    }
                }
                None => {
                    return Err(Error::UnsupportedProtocol(n.to_string()));
                }
            }
        }
        Ok(())
    }
}
