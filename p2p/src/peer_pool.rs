use std::{collections::HashMap, sync::Arc, time::Duration};

use async_channel::Sender;
use bincode::{Decode, Encode};
use log::{error, info, trace, warn};

use karyon_core::{
    async_runtime::{lock::RwLock, Executor},
    async_util::{timeout, TaskGroup, TaskResult},
    util::decode,
};

use karyon_net::{Conn, Endpoint};

use crate::{
    config::Config,
    conn_queue::{ConnDirection, ConnQueue},
    message::{get_msg_payload, NetMsg, NetMsgCmd, VerAckMsg, VerMsg},
    monitor::{Monitor, PPEvent},
    peer::Peer,
    protocol::{Protocol, ProtocolConstructor, ProtocolID},
    protocols::PingProtocol,
    version::{version_match, Version, VersionInt},
    Error, PeerID, Result,
};

pub struct PeerPool {
    /// Peer's ID
    pub id: PeerID,

    /// Connection queue
    conn_queue: Arc<ConnQueue>,

    /// Holds the running peers.
    peers: RwLock<HashMap<PeerID, Arc<Peer>>>,

    /// Hashmap contains protocol constructors.
    pub(crate) protocols: RwLock<HashMap<ProtocolID, Box<ProtocolConstructor>>>,

    /// Hashmap contains protocol IDs and their versions.
    protocol_versions: Arc<RwLock<HashMap<ProtocolID, Version>>>,

    /// Managing spawned tasks.
    task_group: TaskGroup,

    /// A global Executor
    executor: Executor,

    /// The Configuration for the P2P network.
    pub(crate) config: Arc<Config>,

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
        executor: Executor,
    ) -> Arc<Self> {
        let protocols = RwLock::new(HashMap::new());
        let protocol_versions = Arc::new(RwLock::new(HashMap::new()));

        Arc::new(Self {
            id: id.clone(),
            conn_queue,
            peers: RwLock::new(HashMap::new()),
            protocols,
            protocol_versions,
            task_group: TaskGroup::with_executor(executor.clone()),
            executor,
            monitor,
            config,
        })
    }

    /// Starts the [`PeerPool`]
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        self.setup_protocols().await?;
        self.task_group.spawn(
            {
                let this = self.clone();
                async move { this.listen_loop().await }
            },
            |_| async {},
        );
        Ok(())
    }

    /// Shuts down
    pub async fn shutdown(&self) {
        for (_, peer) in self.peers.read().await.iter() {
            peer.shutdown().await;
        }

        self.task_group.cancel().await;
    }

    /// Attach a custom protocol to the network
    pub async fn attach_protocol<P: Protocol>(&self, c: Box<ProtocolConstructor>) -> Result<()> {
        let protocol_versions = &mut self.protocol_versions.write().await;
        let protocols = &mut self.protocols.write().await;

        protocol_versions.insert(P::id(), P::version()?);
        protocols.insert(P::id(), c);
        Ok(())
    }

    /// Broadcast a message to all connected peers using the specified protocol.
    pub async fn broadcast<T: Decode + Encode>(&self, proto_id: &ProtocolID, msg: &T) {
        for (pid, peer) in self.peers.read().await.iter() {
            if let Err(err) = peer.send(proto_id, msg).await {
                error!("failed to send msg to {pid}: {err}");
                continue;
            }
        }
    }

    /// Add a new peer to the peer list.
    pub async fn new_peer(
        self: &Arc<Self>,
        conn: Conn<NetMsg>,
        conn_direction: &ConnDirection,
        disconnect_signal: Sender<Result<()>>,
    ) -> Result<()> {
        let endpoint = conn.peer_endpoint()?;

        // Do a handshake with the connection before creating a new peer.
        let pid = self.do_handshake(&conn, conn_direction).await?;

        // TODO: Consider restricting the subnet for inbound connections
        if self.contains_peer(&pid).await {
            return Err(Error::PeerAlreadyConnected);
        }

        // Create a new peer
        let peer = Peer::new(
            Arc::downgrade(self),
            &pid,
            conn,
            endpoint.clone(),
            conn_direction.clone(),
            self.executor.clone(),
        );

        // Insert the new peer
        self.peers.write().await.insert(pid.clone(), peer.clone());

        let on_disconnect = {
            let this = self.clone();
            let pid = pid.clone();
            |result| async move {
                if let TaskResult::Completed(result) = result {
                    if let Err(err) = this.remove_peer(&pid).await {
                        error!("Failed to remove peer {pid}: {err}");
                    }
                    let _ = disconnect_signal.send(result).await;
                }
            }
        };

        self.task_group.spawn(peer.run(), on_disconnect);

        info!("Add new peer {pid}, direction: {conn_direction}, endpoint: {endpoint}");

        self.monitor.notify(PPEvent::NewPeer(pid.clone())).await;

        Ok(())
    }

    /// Checks if the peer list contains a peer with the given peer id
    pub async fn contains_peer(&self, pid: &PeerID) -> bool {
        self.peers.read().await.contains_key(pid)
    }

    /// Returns the number of currently connected peers.
    pub async fn peers_len(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Returns a map of inbound peers with their endpoints.
    pub async fn inbound_peers(&self) -> HashMap<PeerID, Endpoint> {
        let mut peers = HashMap::new();
        for (id, peer) in self.peers.read().await.iter() {
            if peer.is_inbound() {
                peers.insert(id.clone(), peer.remote_endpoint().clone());
            }
        }
        peers
    }

    /// Returns a map of outbound peers with their endpoints.
    pub async fn outbound_peers(&self) -> HashMap<PeerID, Endpoint> {
        let mut peers = HashMap::new();
        for (id, peer) in self.peers.read().await.iter() {
            if !peer.is_inbound() {
                peers.insert(id.clone(), peer.remote_endpoint().clone());
            }
        }
        peers
    }

    /// Listens to a new connection from the connection queue
    async fn listen_loop(self: Arc<Self>) {
        loop {
            let conn = self.conn_queue.next().await;
            let signal = conn.disconnect_signal;

            let result = self
                .new_peer(conn.conn, &conn.direction, signal.clone())
                .await;

            // Only send a disconnect signal if there is an error when adding a peer.
            if result.is_err() {
                let _ = signal.send(result).await;
            }
        }
    }

    /// Shuts down the peer and remove it from the peer list.
    async fn remove_peer(&self, pid: &PeerID) -> Result<()> {
        let result = self.peers.write().await.remove(pid);

        let peer = match result {
            Some(p) => p,
            None => return Ok(()),
        };

        peer.shutdown().await;

        self.monitor.notify(PPEvent::RemovePeer(pid.clone())).await;

        let endpoint = peer.remote_endpoint();
        let direction = peer.direction();

        warn!("Peer {pid} removed, direction: {direction}, endpoint: {endpoint}",);
        Ok(())
    }

    /// Attach the core protocols.
    async fn setup_protocols(&self) -> Result<()> {
        let executor = self.executor.clone();
        let c = move |peer| PingProtocol::new(peer, executor.clone());
        self.attach_protocol::<PingProtocol>(Box::new(c)).await
    }

    /// Initiate a handshake with a connection.
    async fn do_handshake(
        &self,
        conn: &Conn<NetMsg>,
        conn_direction: &ConnDirection,
    ) -> Result<PeerID> {
        trace!("Handshake started: {}", conn.peer_endpoint()?);
        match conn_direction {
            ConnDirection::Inbound => {
                let result = self.wait_vermsg(conn).await;
                match result {
                    Ok(_) => {
                        self.send_verack(conn, true).await?;
                    }
                    Err(Error::IncompatibleVersion(_)) | Err(Error::UnsupportedProtocol(_)) => {
                        self.send_verack(conn, false).await?;
                    }
                    _ => {}
                }
                result
            }

            ConnDirection::Outbound => {
                self.send_vermsg(conn).await?;
                self.wait_verack(conn).await
            }
        }
    }

    /// Send a Version message
    async fn send_vermsg(&self, conn: &Conn<NetMsg>) -> Result<()> {
        let pids = self.protocol_versions.read().await;
        let protocols = pids.iter().map(|p| (p.0.clone(), p.1.v.clone())).collect();
        drop(pids);

        let vermsg = VerMsg {
            peer_id: self.id.clone(),
            protocols,
            version: self.config.version.v.clone(),
        };

        trace!("Send VerMsg");
        conn.send(NetMsg::new(NetMsgCmd::Version, &vermsg)?).await?;
        Ok(())
    }

    /// Wait for a Version message
    ///
    /// Returns the peer's ID upon successfully receiving the Version message.
    async fn wait_vermsg(&self, conn: &Conn<NetMsg>) -> Result<PeerID> {
        let t = Duration::from_secs(self.config.handshake_timeout);
        let msg: NetMsg = timeout(t, conn.recv()).await??;

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
    async fn send_verack(&self, conn: &Conn<NetMsg>, ack: bool) -> Result<()> {
        let verack = VerAckMsg {
            peer_id: self.id.clone(),
            ack,
        };

        trace!("Send VerAckMsg {:?}", verack);
        conn.send(NetMsg::new(NetMsgCmd::Verack, &verack)?).await?;
        Ok(())
    }

    /// Wait for a Verack message
    ///
    /// Returns the peer's ID upon successfully receiving the Verack message.
    async fn wait_verack(&self, conn: &Conn<NetMsg>) -> Result<PeerID> {
        let t = Duration::from_secs(self.config.handshake_timeout);
        let msg: NetMsg = timeout(t, conn.recv()).await??;

        let payload = get_msg_payload!(Verack, msg);
        let (verack, _) = decode::<VerAckMsg>(&payload)?;

        if !verack.ack {
            return Err(Error::IncompatiblePeer);
        }

        trace!("Received VerAckMsg from: {}", verack.peer_id);
        Ok(verack.peer_id)
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
