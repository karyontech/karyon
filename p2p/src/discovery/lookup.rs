use std::{sync::Arc, time::Duration};

use futures_util::stream::{FuturesUnordered, StreamExt};
use log::{error, trace};
use parking_lot::RwLock;
use rand::{rngs::OsRng, seq::SliceRandom, RngCore};

use karyon_core::{async_runtime::Executor, async_util::timeout, crypto::KeyPair, util::decode};

use karyon_net::Endpoint;

use crate::{
    connector::Connector,
    listener::Listener,
    message::{FindPeerMsg, NetMsg, NetMsgCmd, PeerMsg, PeersMsg, PingMsg, PongMsg, ShutdownMsg},
    monitor::{ConnEvent, DiscvEvent, Monitor},
    routing_table::RoutingTable,
    slots::ConnectionSlots,
    version::version_match,
    Config, ConnRef, Error, PeerID, Result,
};

/// Maximum number of peers that can be returned in a PeersMsg.
pub const MAX_PEERS_IN_PEERSMSG: usize = 10;

pub struct LookupService {
    /// Peer's ID
    id: PeerID,

    /// Routing Table
    table: Arc<RoutingTable>,

    /// Listener
    listener: Arc<Listener>,
    /// Connector
    connector: Arc<Connector>,

    /// Outbound slots.
    outbound_slots: Arc<ConnectionSlots>,

    /// Resolved listen endpoint
    listen_endpoint: RwLock<Option<Endpoint>>,

    /// Resolved discovery endpoint
    discovery_endpoint: RwLock<Option<Endpoint>>,

    /// Holds the configuration for the P2P network.
    config: Arc<Config>,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,
}

impl LookupService {
    /// Creates a new lookup service
    pub fn new(
        key_pair: &KeyPair,
        table: Arc<RoutingTable>,
        config: Arc<Config>,
        monitor: Arc<Monitor>,
        ex: Executor,
    ) -> Self {
        let inbound_slots = Arc::new(ConnectionSlots::new(config.lookup_inbound_slots));
        let outbound_slots = Arc::new(ConnectionSlots::new(config.lookup_outbound_slots));

        let listener = Listener::new(
            key_pair,
            inbound_slots.clone(),
            config.enable_tls,
            monitor.clone(),
            ex.clone(),
        );

        let connector = Connector::new(
            key_pair,
            config.lookup_connect_retries,
            outbound_slots.clone(),
            config.enable_tls,
            monitor.clone(),
            ex,
        );

        let id = key_pair
            .public()
            .try_into()
            .expect("Get PeerID from KeyPair");
        Self {
            id,
            table,
            listener,
            connector,
            outbound_slots,
            listen_endpoint: RwLock::new(None),
            discovery_endpoint: RwLock::new(None),
            config,
            monitor,
        }
    }

    /// Start the lookup service.
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        self.start_listener().await?;
        Ok(())
    }

    /// Set the resolved listen endpoint.
    pub fn set_listen_endpoint(&self, resolved_endpoint: &Endpoint) -> Result<()> {
        let discovery_endpoint = Endpoint::Tcp(
            resolved_endpoint.addr()?.clone(),
            self.config.discovery_port,
        );
        *self.listen_endpoint.write() = Some(resolved_endpoint.clone());
        *self.discovery_endpoint.write() = Some(discovery_endpoint.clone());
        Ok(())
    }

    pub fn listen_endpoint(&self) -> Option<Endpoint> {
        self.listen_endpoint.read().clone()
    }

    pub fn discovery_endpoint(&self) -> Option<Endpoint> {
        self.discovery_endpoint.read().clone()
    }

    /// Shuts down the lookup service.
    pub async fn shutdown(&self) {
        self.connector.shutdown().await;
        self.listener.shutdown().await;
    }

    /// Starts iterative lookup and populate the routing table.
    ///
    /// This method begins by generating a random peer ID and connecting to the
    /// provided endpoint. It then sends a FindPeer message containing the
    /// randomly generated peer ID. Upon receiving peers from the initial lookup,
    /// it starts connecting to these received peers and sends them a FindPeer
    /// message that contains our own peer ID.
    pub async fn start_lookup(&self, endpoint: &Endpoint, peer_id: Option<PeerID>) -> Result<()> {
        trace!("Lookup started {endpoint}");
        self.monitor
            .notify(DiscvEvent::LookupStarted(endpoint.clone()))
            .await;

        let mut random_peers = vec![];
        if let Err(err) = self
            .random_lookup(endpoint, peer_id, &mut random_peers)
            .await
        {
            self.monitor
                .notify(DiscvEvent::LookupFailed(endpoint.clone()))
                .await;
            return Err(err);
        };

        let mut peer_buffer = vec![];
        if let Err(err) = self.self_lookup(&random_peers, &mut peer_buffer).await {
            self.monitor
                .notify(DiscvEvent::LookupFailed(endpoint.clone()))
                .await;
            return Err(err);
        }

        while peer_buffer.len() < MAX_PEERS_IN_PEERSMSG {
            match random_peers.pop() {
                Some(p) => peer_buffer.push(p),
                None => break,
            }
        }

        for peer in peer_buffer.iter() {
            let result = self.table.add_entry(peer.clone().into());
            trace!("Add entry {:?}", result);
        }

        self.monitor
            .notify(DiscvEvent::LookupSucceeded(
                endpoint.clone(),
                peer_buffer.len(),
            ))
            .await;

        Ok(())
    }

    /// Starts a random lookup
    ///
    /// This will perfom lookup on a random generated PeerID
    async fn random_lookup(
        &self,
        endpoint: &Endpoint,
        peer_id: Option<PeerID>,
        random_peers: &mut Vec<PeerMsg>,
    ) -> Result<()> {
        for _ in 0..2 {
            let random_peer_id = PeerID::random();
            let peers = self
                .connect(endpoint.clone(), peer_id.clone(), &random_peer_id)
                .await?;

            for peer in peers {
                if random_peers.contains(&peer)
                    || peer.peer_id == self.id
                    || self.table.contains_key(&peer.peer_id.0)
                {
                    continue;
                }

                random_peers.push(peer);
            }
        }

        Ok(())
    }

    /// Starts a self lookup
    async fn self_lookup(
        &self,
        random_peers: &[PeerMsg],
        peer_buffer: &mut Vec<PeerMsg>,
    ) -> Result<()> {
        let mut results = FuturesUnordered::new();
        for peer in random_peers.choose_multiple(&mut OsRng, random_peers.len()) {
            let endpoint = Endpoint::Tcp(peer.addr.clone(), peer.discovery_port);
            results.push(self.connect(endpoint, Some(peer.peer_id.clone()), &self.id))
        }

        while let Some(result) = results.next().await {
            match result {
                Ok(peers) => peer_buffer.extend(peers),
                Err(err) => {
                    error!("Failed to do self lookup: {err}");
                }
            }
        }

        Ok(())
    }

    /// Connects to the given endpoint and initiates a lookup process for the
    /// provided peer ID.
    async fn connect(
        &self,
        endpoint: Endpoint,
        peer_id: Option<PeerID>,
        target_peer_id: &PeerID,
    ) -> Result<Vec<PeerMsg>> {
        let conn = self.connector.connect(&endpoint, &peer_id).await?;
        let result = self.handle_outbound(conn, target_peer_id).await;

        self.monitor.notify(ConnEvent::Disconnected(endpoint)).await;
        self.outbound_slots.remove().await;

        result
    }

    /// Handles outbound connection
    async fn handle_outbound(
        &self,
        conn: ConnRef,
        target_peer_id: &PeerID,
    ) -> Result<Vec<PeerMsg>> {
        trace!("Send Ping msg");
        let peers;

        let ping_msg = self.send_ping_msg(&conn).await?;

        loop {
            let t = Duration::from_secs(self.config.lookup_response_timeout);
            let msg: NetMsg = timeout(t, conn.recv()).await??;
            match msg.header.command {
                NetMsgCmd::Pong => {
                    let (pong_msg, _) = decode::<PongMsg>(&msg.payload)?;
                    if ping_msg.nonce != pong_msg.0 {
                        return Err(Error::InvalidPongMsg);
                    }
                    trace!("Send FindPeer msg");
                    self.send_findpeer_msg(&conn, target_peer_id).await?;
                }
                NetMsgCmd::Peers => {
                    peers = decode::<PeersMsg>(&msg.payload)?.0.peers;
                    if peers.len() >= MAX_PEERS_IN_PEERSMSG {
                        return Err(Error::Lookup("Received too many peers in PeersMsg"));
                    }
                    break;
                }
                c => return Err(Error::InvalidMsg(format!("Unexpected msg: {:?}", c))),
            };
        }

        trace!("Send Peer msg");
        if let Some(endpoint) = self.listen_endpoint() {
            self.send_peer_msg(&conn, endpoint).await?;
        }

        trace!("Send Shutdown msg");
        self.send_shutdown_msg(&conn).await?;

        Ok(peers)
    }

    /// Start a listener.
    async fn start_listener(self: &Arc<Self>) -> Result<()> {
        let endpoint: Endpoint = match self.discovery_endpoint() {
            Some(e) => e,
            None => return Ok(()),
        };

        let callback = {
            let this = self.clone();
            |conn: ConnRef| async move {
                let t = Duration::from_secs(this.config.lookup_connection_lifespan);
                timeout(t, this.handle_inbound(conn)).await??;
                Ok(())
            }
        };

        self.listener.start(endpoint, callback).await?;
        Ok(())
    }

    /// Handles inbound connection
    async fn handle_inbound(self: &Arc<Self>, conn: ConnRef) -> Result<()> {
        loop {
            let msg: NetMsg = conn.recv().await?;
            trace!("Receive msg {:?}", msg.header.command);

            if let NetMsgCmd::Shutdown = msg.header.command {
                return Ok(());
            }

            match &msg.header.command {
                NetMsgCmd::Ping => {
                    let (ping_msg, _) = decode::<PingMsg>(&msg.payload)?;
                    if !version_match(&self.config.version.req, &ping_msg.version) {
                        return Err(Error::IncompatibleVersion("system: {}".into()));
                    }
                    self.send_pong_msg(ping_msg.nonce, &conn).await?;
                }
                NetMsgCmd::FindPeer => {
                    let (findpeer_msg, _) = decode::<FindPeerMsg>(&msg.payload)?;
                    let peer_id = findpeer_msg.0;
                    self.send_peers_msg(&peer_id, &conn).await?;
                }
                NetMsgCmd::Peer => {
                    let (peer, _) = decode::<PeerMsg>(&msg.payload)?;
                    let result = self.table.add_entry(peer.clone().into());
                    trace!("Add entry result: {:?}", result);
                }
                c => return Err(Error::InvalidMsg(format!("Unexpected msg: {:?}", c))),
            }
        }
    }

    /// Sends a Ping msg.
    async fn send_ping_msg(&self, conn: &ConnRef) -> Result<PingMsg> {
        trace!("Send Pong msg");
        let mut nonce: [u8; 32] = [0; 32];
        RngCore::fill_bytes(&mut OsRng, &mut nonce);

        let ping_msg = PingMsg {
            version: self.config.version.v.clone(),
            nonce,
        };
        conn.send(NetMsg::new(NetMsgCmd::Ping, &ping_msg)?).await?;
        Ok(ping_msg)
    }

    /// Sends a Pong msg
    async fn send_pong_msg(&self, nonce: [u8; 32], conn: &ConnRef) -> Result<()> {
        trace!("Send Pong msg");
        conn.send(NetMsg::new(NetMsgCmd::Pong, PongMsg(nonce))?)
            .await?;
        Ok(())
    }

    /// Sends a FindPeer msg
    async fn send_findpeer_msg(&self, conn: &ConnRef, peer_id: &PeerID) -> Result<()> {
        trace!("Send FindPeer msg");
        conn.send(NetMsg::new(
            NetMsgCmd::FindPeer,
            FindPeerMsg(peer_id.clone()),
        )?)
        .await?;
        Ok(())
    }

    /// Sends a Peers msg.
    async fn send_peers_msg(&self, peer_id: &PeerID, conn: &ConnRef) -> Result<()> {
        trace!("Send Peers msg");
        let entries = self
            .table
            .closest_entries(&peer_id.0, MAX_PEERS_IN_PEERSMSG);

        let peers: Vec<PeerMsg> = entries.into_iter().map(|e| e.into()).collect();
        conn.send(NetMsg::new(NetMsgCmd::Peers, PeersMsg { peers })?)
            .await?;
        Ok(())
    }

    /// Sends a Peer msg.
    async fn send_peer_msg(&self, conn: &ConnRef, endpoint: Endpoint) -> Result<()> {
        trace!("Send Peer msg");
        let peer_msg = PeerMsg {
            addr: endpoint.addr()?.clone(),
            port: *endpoint.port()?,
            discovery_port: self.config.discovery_port,
            peer_id: self.id.clone(),
        };
        conn.send(NetMsg::new(NetMsgCmd::Peer, &peer_msg)?).await?;
        Ok(())
    }

    /// Sends a Shutdown msg.
    async fn send_shutdown_msg(&self, conn: &ConnRef) -> Result<()> {
        trace!("Send Shutdown msg");
        conn.send(NetMsg::new(NetMsgCmd::Shutdown, ShutdownMsg(0))?)
            .await?;
        Ok(())
    }
}
