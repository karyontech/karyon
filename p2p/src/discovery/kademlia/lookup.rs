use std::{sync::Arc, time::Duration};

use futures_util::stream::{FuturesUnordered, StreamExt};
use log::{error, trace};
use rand::{rngs::OsRng, seq::SliceRandom, RngCore};

use karyon_core::{async_runtime::Executor, async_util::timeout, crypto::KeyPair};

use karyon_net::Endpoint;

use crate::{
    bloom::BloomRef,
    connector::Connector,
    discovery::kademlia::{
        messages::{
            FindPeerMsg, KadNetCmd, KadNetMsg, KadNetMsgCodec, PeerMsg, PeersMsg, PingMsg, PongMsg,
        },
        routing_table::RoutingTable,
    },
    listener::Listener,
    message::{pick_endpoint, PeerAddr, Protocol, ShutdownMsg},
    monitor::{ConnectionKind, DiscoveryKind, Monitor},
    slots::ConnectionSlots,
    util::decode,
    version::version_match,
    Config, Error, PeerID, Result,
};

/// Framed lookup-plane connection.
type KadConnRef = karyon_net::FramedConn<KadNetMsgCodec>;

/// Maximum number of peers that can be returned in a PeersMsg.
pub const MAX_PEERS_IN_PEERSMSG: usize = 10;

/// Maximum data-plane addresses a peer may advertise about itself.
/// Legitimate peers only need one per transport (tcp/tls/quic).
pub const MAX_ADDRS_PER_PEER: usize = 4;

/// Maximum discovery addresses a peer may advertise about itself.
/// Legitimate peers only need lookup + refresh.
pub const MAX_DISCOVERY_ADDRS_PER_PEER: usize = 3;

/// Endpoints the lookup service advertises and binds to.
pub struct LookupEndpoints {
    /// Data-plane listen addrs (advertised in PeerMsg.addrs).
    pub listen: Vec<Endpoint>,
    /// Local bind for the lookup listener (also advertised).
    pub lookup: Option<Endpoint>,
    /// UDP refresh addr to advertise (if any).
    pub refresh: Option<Endpoint>,
}

pub struct LookupService {
    /// Peer's ID
    id: PeerID,

    /// Routing Table
    table: Arc<RoutingTable>,

    /// Listener
    listener: Arc<Listener<KadNetMsgCodec>>,
    /// Connector
    connector: Arc<Connector<KadNetMsgCodec>>,

    /// Outbound slots.
    outbound_slots: Arc<ConnectionSlots>,

    /// Endpoints this service advertises and binds to.
    endpoints: LookupEndpoints,

    /// Holds the configuration for the P2P network.
    config: Arc<Config>,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,

    /// Shared local bloom. Snapshotted on every outgoing PeerMsg.
    bloom: BloomRef,
}

impl LookupService {
    /// Creates a new lookup service.
    pub fn new(
        key_pair: &KeyPair,
        table: Arc<RoutingTable>,
        config: Arc<Config>,
        monitor: Arc<Monitor>,
        bloom: BloomRef,
        endpoints: LookupEndpoints,
        ex: Executor,
    ) -> Self {
        let inbound_slots = Arc::new(ConnectionSlots::new(config.lookup_inbound_slots));
        let outbound_slots = Arc::new(ConnectionSlots::new(config.lookup_outbound_slots));

        let listener = Listener::new(key_pair, inbound_slots.clone(), monitor.clone(), ex.clone());

        let connector = Connector::new(
            key_pair,
            config.lookup_connect_retries,
            outbound_slots.clone(),
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
            endpoints,
            config,
            monitor,
            bloom,
        }
    }

    /// Start the lookup service.
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        self.start_listener().await?;
        Ok(())
    }

    pub fn lookup_endpoint(&self) -> Option<&Endpoint> {
        self.endpoints.lookup.as_ref()
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
            .notify(DiscoveryKind::LookupStarted(endpoint.clone()))
            .await;

        let mut random_peers = vec![];
        if let Err(err) = self
            .random_lookup(endpoint, peer_id, &mut random_peers)
            .await
        {
            self.monitor
                .notify(DiscoveryKind::LookupFailed(endpoint.clone()))
                .await;
            return Err(err);
        };

        let mut peer_buffer = vec![];
        if let Err(err) = self.self_lookup(&random_peers, &mut peer_buffer).await {
            self.monitor
                .notify(DiscoveryKind::LookupFailed(endpoint.clone()))
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
            trace!("Add entry {result:?}");
        }

        self.monitor
            .notify(DiscoveryKind::LookupSucceeded(
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
        #[cfg(feature = "quic")]
        let supported = [Protocol::Tcp, Protocol::Quic];
        #[cfg(not(feature = "quic"))]
        let supported = [Protocol::Tcp];
        for peer in random_peers.choose_multiple(&mut OsRng, random_peers.len()) {
            let endpoint = match pick_endpoint(&peer.discovery_addrs, &supported) {
                Some(ep) => ep,
                None => continue,
            };
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

        self.monitor
            .notify(ConnectionKind::Disconnected(endpoint))
            .await;
        self.outbound_slots.remove().await;

        result
    }

    /// Handles outbound connection
    async fn handle_outbound(
        &self,
        mut conn: KadConnRef,
        target_peer_id: &PeerID,
    ) -> Result<Vec<PeerMsg>> {
        trace!("Send Ping msg");
        let mut peers;

        let ping_msg = self.send_ping_msg(&mut conn).await?;

        loop {
            let t = Duration::from_secs(self.config.lookup_response_timeout);
            let msg: KadNetMsg = timeout(t, conn.recv_msg()).await??;
            match msg.header.command {
                KadNetCmd::Pong => {
                    let (pong_msg, _) = decode::<PongMsg>(&msg.payload)?;
                    if ping_msg.nonce != pong_msg.0 {
                        return Err(Error::InvalidPongMsg);
                    }
                    trace!("Send FindPeer msg");
                    self.send_findpeer_msg(&mut conn, target_peer_id).await?;
                }
                KadNetCmd::Peers => {
                    peers = decode::<PeersMsg>(&msg.payload)?.0.peers;
                    if peers.len() > MAX_PEERS_IN_PEERSMSG {
                        return Err(Error::Lookup(
                            "Received too many peers in PeersMsg".to_string(),
                        ));
                    }
                    for p in &mut peers {
                        validate_peer_msg(p)?;
                    }
                    break;
                }
                c => return Err(Error::InvalidMsg(format!("Unexpected msg: {c:?}"))),
            };
        }

        trace!("Send Peer msg");
        self.send_peer_msg(&mut conn).await?;

        trace!("Send Shutdown msg");
        self.send_shutdown_msg(&mut conn).await?;

        Ok(peers)
    }

    /// Start a listener.
    async fn start_listener(self: &Arc<Self>) -> Result<()> {
        let endpoint = match self.lookup_endpoint() {
            Some(e) => e.clone(),
            None => return Ok(()),
        };

        if !endpoint.is_tcp() {
            return Err(Error::Config(format!(
                "lookup endpoint must be tcp://..., got {endpoint}"
            )));
        }

        let callback = {
            let this = self.clone();
            |conn: KadConnRef| async move {
                let t = Duration::from_secs(this.config.lookup_connection_lifespan);
                timeout(t, this.handle_inbound(conn)).await??;
                Ok(())
            }
        };

        self.listener
            .start_with_callback(endpoint, callback)
            .await?;
        Ok(())
    }

    /// Handles inbound connection
    async fn handle_inbound(self: &Arc<Self>, mut conn: KadConnRef) -> Result<()> {
        loop {
            let msg: KadNetMsg = conn.recv_msg().await?;
            trace!("Receive msg {:?}", msg.header.command);

            if let KadNetCmd::Shutdown = msg.header.command {
                return Ok(());
            }

            match &msg.header.command {
                KadNetCmd::Ping => {
                    let (ping_msg, _) = decode::<PingMsg>(&msg.payload)?;
                    if !version_match(&self.config.version.req, &ping_msg.version) {
                        return Err(Error::IncompatibleVersion("system: {}".into()));
                    }
                    self.send_pong_msg(ping_msg.nonce, &mut conn).await?;
                }
                KadNetCmd::FindPeer => {
                    let (findpeer_msg, _) = decode::<FindPeerMsg>(&msg.payload)?;
                    let peer_id = findpeer_msg.0;
                    self.send_peers_msg(&peer_id, &mut conn).await?;
                }
                KadNetCmd::Peer => {
                    let (mut peer, _) = decode::<PeerMsg>(&msg.payload)?;
                    validate_peer_msg(&mut peer)?;
                    let result = self.table.add_entry(peer.into());
                    trace!("Add entry result: {result:?}");
                }
                c => return Err(Error::InvalidMsg(format!("Unexpected msg: {c:?}"))),
            }
        }
    }

    /// Sends a Ping msg.
    async fn send_ping_msg(&self, conn: &mut KadConnRef) -> Result<PingMsg> {
        trace!("Send Pong msg");
        let mut nonce: [u8; 32] = [0; 32];
        RngCore::fill_bytes(&mut OsRng, &mut nonce);

        let ping_msg = PingMsg {
            version: self.config.version.v.clone(),
            nonce,
        };
        conn.send_msg(KadNetMsg::new(KadNetCmd::Ping, &ping_msg)?)
            .await?;
        Ok(ping_msg)
    }

    /// Sends a Pong msg
    async fn send_pong_msg(&self, nonce: [u8; 32], conn: &mut KadConnRef) -> Result<()> {
        trace!("Send Pong msg");
        conn.send_msg(KadNetMsg::new(KadNetCmd::Pong, PongMsg(nonce))?)
            .await?;
        Ok(())
    }

    /// Sends a FindPeer msg
    async fn send_findpeer_msg(&self, conn: &mut KadConnRef, peer_id: &PeerID) -> Result<()> {
        trace!("Send FindPeer msg");
        conn.send_msg(KadNetMsg::new(
            KadNetCmd::FindPeer,
            FindPeerMsg(peer_id.clone()),
        )?)
        .await?;
        Ok(())
    }

    /// Sends a Peers msg.
    async fn send_peers_msg(&self, peer_id: &PeerID, conn: &mut KadConnRef) -> Result<()> {
        trace!("Send Peers msg");
        let entries = self
            .table
            .closest_entries(&peer_id.0, MAX_PEERS_IN_PEERSMSG);

        let peers: Vec<PeerMsg> = entries.into_iter().map(|e| e.into()).collect();
        conn.send_msg(KadNetMsg::new(KadNetCmd::Peers, PeersMsg { peers })?)
            .await?;
        Ok(())
    }

    /// Sends a Peer msg advertising our listen and discovery addresses.
    /// `addrs` carries every data-plane listen endpoint; `discovery_addrs`
    /// carries the lookup endpoint and (when set) the udp refresh endpoint.
    async fn send_peer_msg(&self, conn: &mut KadConnRef) -> Result<()> {
        trace!("Send Peer msg");

        let mut addrs = Vec::new();
        for ep in &self.endpoints.listen {
            if let Some(pa) = PeerAddr::from_endpoint(ep, 0) {
                addrs.push(pa);
            }
        }

        let mut discovery_addrs = Vec::new();
        if let Some(ep) = self.endpoints.lookup.as_ref() {
            if let Some(pa) = PeerAddr::from_endpoint(ep, 0) {
                discovery_addrs.push(pa);
            }
        }
        if let Some(ep) = self.endpoints.refresh.as_ref() {
            if let Some(pa) = PeerAddr::from_endpoint(ep, 0) {
                discovery_addrs.push(pa);
            }
        }

        let peer_msg = PeerMsg {
            peer_id: self.id.clone(),
            addrs,
            discovery_addrs,
            protocols: *self.bloom.read(),
        };
        conn.send_msg(KadNetMsg::new(KadNetCmd::Peer, &peer_msg)?)
            .await?;
        Ok(())
    }

    /// Sends a Shutdown msg.
    async fn send_shutdown_msg(&self, conn: &mut KadConnRef) -> Result<()> {
        trace!("Send Shutdown msg");
        conn.send_msg(KadNetMsg::new(KadNetCmd::Shutdown, ShutdownMsg(0))?)
            .await?;
        Ok(())
    }
}

/// Reject PeerMsgs that advertise more addresses than a legitimate
/// peer would. Caps memory blow-up from malicious or buggy peers.
fn validate_peer_msg(p: &mut PeerMsg) -> Result<()> {
    if p.addrs.len() > MAX_ADDRS_PER_PEER {
        return Err(Error::InvalidMsg(format!(
            "PeerMsg.addrs has {} entries, max {MAX_ADDRS_PER_PEER}",
            p.addrs.len()
        )));
    }
    p.discovery_addrs
        .retain(|a| !matches!(a.protocol, Protocol::Tls));
    if p.discovery_addrs.len() > MAX_DISCOVERY_ADDRS_PER_PEER {
        return Err(Error::InvalidMsg(format!(
            "PeerMsg.discovery_addrs has {} entries, max {MAX_DISCOVERY_ADDRS_PER_PEER}",
            p.discovery_addrs.len()
        )));
    }
    Ok(())
}
