mod swarm_key;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use log::{debug, trace};

use karyon_core::{
    async_runtime::{lock::RwLock, Executor},
    async_util::{TaskGroup, TaskResult},
};

use karyon_p2p::{
    protocol::{PeerConn, Protocol, ProtocolID},
    DiscoveredPeer, Node, PeerEvent, PeerID, Result,
};

pub use swarm_key::{compute_swarm_key, swarm_key_from_protocol, SwarmKey};

/// Per-swarm state.
struct SwarmInfo {
    /// The protocol id this swarm wraps. The instance is captured by
    /// the `SwarmKey`; the protocol id is what's negotiated at handshake.
    protocol_id: ProtocolID,
    /// Connected peers currently in this swarm.
    peers: RwLock<HashSet<PeerID>>,
}

/// Swarm layer on top of Node. Manages protocol-aware peer groups
/// and advertises swarm membership in the Node's bloom filter.
///
/// Each call to `join` registers a new swarm; a single `Swarm` instance
/// can manage many concurrent swarms across different protocols (and
/// optionally different instances per protocol). By default a swarm's
/// [`SwarmKey`] is derived from the protocol id alone; use
/// [`Swarm::join_with_instance`] for sub-grouping inside a single
/// protocol (e.g. chat rooms, pub/sub topics).
///
/// Joining a swarm also adds the swarm key to the Node's optional
/// bloom, so other peers can discover this node via
/// [`Node::find_peers_with`] / [`Swarm::find_peers`] without paying
/// the cost of a full handshake first.
pub struct Swarm {
    node: Arc<Node>,
    /// Swarm key -> swarm state.
    swarms: RwLock<HashMap<SwarmKey, SwarmInfo>>,
    /// Reverse index: peer -> set of swarm keys it currently belongs to.
    /// Used for O(1) cleanup on disconnect.
    peer_swarms: RwLock<HashMap<PeerID, HashSet<SwarmKey>>>,
    task_group: TaskGroup,
}

impl Swarm {
    /// Wraps the given Node.
    pub fn new(node: Arc<Node>, executor: Executor) -> Arc<Self> {
        Arc::new(Self {
            node,
            swarms: RwLock::new(HashMap::new()),
            peer_swarms: RwLock::new(HashMap::new()),
            task_group: TaskGroup::with_executor(executor),
        })
    }

    /// Run the node and start tracking peer events for swarm membership.
    pub async fn run(self: &Arc<Self>) -> Result<()> {
        self.node.run().await?;

        let this = self.clone();
        self.task_group.spawn(
            async move { this.monitor_peers().await },
            |res: TaskResult<Result<()>>| async move {
                debug!("Swarm monitor_peers task ended: {res}");
            },
        );

        Ok(())
    }

    /// Register a new swarm whose [`SwarmKey`] is derived from the
    /// protocol id. The same `Swarm` can hold many of these for
    /// different protocols.
    pub async fn join<P: Protocol>(
        self: &Arc<Self>,
        c: impl Fn(PeerConn) -> Result<Arc<dyn Protocol>> + Send + Sync + 'static,
    ) -> Result<SwarmKey> {
        let proto_id = P::id();
        let key = swarm_key_from_protocol(&proto_id);
        self.join_inner::<P>(proto_id, key, c).await
    }

    /// Join a swarm with an explicit instance name (e.g. a chat room or
    /// pub/sub topic). SwarmKey is derived from `(protocol_id, instance)`.
    pub async fn join_with_instance<P: Protocol>(
        self: &Arc<Self>,
        instance: &str,
        c: impl Fn(PeerConn) -> Result<Arc<dyn Protocol>> + Send + Sync + 'static,
    ) -> Result<SwarmKey> {
        let proto_id = P::id();
        let key = compute_swarm_key(&proto_id, instance);
        self.join_inner::<P>(proto_id, key, c).await
    }

    /// Shared join logic: register the protocol, advertise the swarm
    /// key in the optional bloom, and start tracking connected peers.
    async fn join_inner<P: Protocol>(
        self: &Arc<Self>,
        proto_id: ProtocolID,
        key: SwarmKey,
        c: impl Fn(PeerConn) -> Result<Arc<dyn Protocol>> + Send + Sync + 'static,
    ) -> Result<SwarmKey> {
        // Protocol attach is idempotent in spirit but the underlying
        // peer_pool stores the latest constructor; calling join twice
        // for the same protocol just updates the constructor. Bloom
        // adds are also idempotent (bits already set).
        self.node.attach_protocol::<P>(c).await?;
        self.node.bloom_add_optional(key);

        let info = SwarmInfo {
            protocol_id: proto_id.clone(),
            peers: RwLock::new(HashSet::new()),
        };
        self.swarms.write().await.insert(key, info);

        // Existing connected peers may already speak this protocol.
        self.scan_peers_for_swarm(&key, &proto_id).await;

        debug!("Joined swarm {proto_id}");
        Ok(key)
    }

    /// Leave a swarm. Removes local membership tracking. The protocol
    /// stays registered on the node (protocols are permanent), and
    /// the bloom bit stays set (blooms can't unset bits without rebuilding).
    pub async fn leave(&self, key: &SwarmKey) {
        self.swarms.write().await.remove(key);

        let mut peer_swarms = self.peer_swarms.write().await;
        for (_, keys) in peer_swarms.iter_mut() {
            keys.remove(key);
        }
    }

    /// Broadcast a message to all connected peers in this swarm.
    pub async fn broadcast(&self, key: &SwarmKey, msg: Vec<u8>) {
        let swarms = self.swarms.read().await;
        let info = match swarms.get(key) {
            Some(i) => i,
            None => return,
        };

        let targets = info.peers.read().await.clone();
        let proto_id = info.protocol_id.clone();
        drop(swarms);

        self.node.broadcast_to(&proto_id, msg, &targets).await;
    }

    /// Connected peer ids in this swarm.
    pub async fn peers(&self, key: &SwarmKey) -> HashSet<PeerID> {
        let swarms = self.swarms.read().await;
        match swarms.get(key) {
            Some(info) => info.peers.read().await.clone(),
            None => HashSet::new(),
        }
    }

    /// Number of connected peers in this swarm.
    pub async fn peers_len(&self, key: &SwarmKey) -> usize {
        let swarms = self.swarms.read().await;
        match swarms.get(key) {
            Some(info) => info.peers.read().await.len(),
            None => 0,
        }
    }

    /// Routing-table candidates whose advertised bloom may contain the
    /// swarm key. False positives are possible (bloom semantics) - the
    /// returned peers may not actually be in the swarm. Use this to
    /// trigger swarm-targeted dials before any handshake has happened.
    pub fn find_peers(&self, key: &SwarmKey) -> Vec<DiscoveredPeer> {
        self.node.find_peers_with(key)
    }

    /// Reference to the underlying node.
    pub fn node(&self) -> &Arc<Node> {
        &self.node
    }

    /// Shut down the swarm and the node.
    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
        self.node.shutdown().await;
    }

    /// Forward Node peer events into per-swarm membership.
    async fn monitor_peers(self: Arc<Self>) -> Result<()> {
        let listener = self.node.register_peer_events();
        loop {
            let event = match listener.recv().await {
                Ok(e) => e,
                Err(err) => {
                    debug!("Swarm peer-event listener closed: {err}");
                    break;
                }
            };
            match event {
                PeerEvent::Added(pid) => self.on_peer_added(&pid).await,
                PeerEvent::Removed(pid) => self.on_peer_removed(&pid).await,
                PeerEvent::HandshakeFailed(_) => {}
            }
        }
        Ok(())
    }

    /// Add the peer to every swarm whose protocol it negotiated.
    async fn on_peer_added(&self, pid: &PeerID) {
        let protos = match self.node.peer_protocol_set(pid).await {
            Some(p) => p,
            None => return,
        };

        let swarms = self.swarms.read().await;
        let mut joined_keys = HashSet::new();
        for (key, info) in swarms.iter() {
            if protos.contains(&info.protocol_id) {
                info.peers.write().await.insert(pid.clone());
                joined_keys.insert(*key);
                trace!("Peer {pid} joined swarm {:?}", key);
            }
        }

        if !joined_keys.is_empty() {
            self.peer_swarms
                .write()
                .await
                .insert(pid.clone(), joined_keys);
        }
    }

    /// Remove the peer from every swarm it was in.
    async fn on_peer_removed(&self, pid: &PeerID) {
        let keys = self.peer_swarms.write().await.remove(pid);
        if let Some(keys) = keys {
            let swarms = self.swarms.read().await;
            for key in &keys {
                if let Some(info) = swarms.get(key) {
                    info.peers.write().await.remove(pid);
                    trace!("Peer {pid} left swarm {:?}", key);
                }
            }
        }
    }

    /// Scan currently connected peers and add the ones speaking
    /// `protocol_id` to the freshly-joined swarm.
    async fn scan_peers_for_swarm(&self, key: &SwarmKey, protocol_id: &ProtocolID) {
        let inbound = self.node.inbound_peers().await;
        let outbound = self.node.outbound_peers().await;
        let all_pids: Vec<PeerID> = inbound.keys().chain(outbound.keys()).cloned().collect();

        let swarms = self.swarms.read().await;
        let info = match swarms.get(key) {
            Some(i) => i,
            None => return,
        };

        for pid in all_pids {
            if let Some(protos) = self.node.peer_protocol_set(&pid).await {
                if protos.contains(protocol_id) {
                    info.peers.write().await.insert(pid.clone());
                    self.peer_swarms
                        .write()
                        .await
                        .entry(pid)
                        .or_default()
                        .insert(*key);
                }
            }
        }
    }
}
