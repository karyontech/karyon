use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use log::{debug, error, info};

use karyon_core::{
    async_runtime::Executor,
    async_util::{TaskGroup, TaskResult},
    crypto::KeyPair,
};
use karyon_eventemitter::EventListener;
use karyon_net::Endpoint;

use crate::{
    PeerID, Result,
    access_control::{Action, Subject},
    codec::PeerNetMsgCodec,
    config::Config,
    conn_direction::ConnDirection,
    conn_queue::ConnQueue,
    connector::Connector,
    discovery::{
        DiscoveredPeer, Discovery, DiscoveryContext, PeerConnectionEvent, StaticDiscovery,
    },
    listener::Listener,
    message::{Protocol, pick_endpoint},
    monitor::{Monitor, PoolEvent},
    peer_pool::{PeerEvent, PeerEventTopic, PeerPool},
    protocol::{PeerConn, Protocol as ProtocolTrait, ProtocolFlags, ProtocolID},
    protocols::PingProtocol,
};

/// Central entry point for the p2p network.
///
/// Manages peer connections, discovery, and protocol registration.
///
/// # Example
///
/// ```no_run
/// use karyon_core::async_runtime::global_executor;
/// use karyon_p2p::{Node, Config, keypair::{KeyPair, KeyPairType}};
///
/// let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
/// let config = Config {
///     listen_endpoints: vec![
///         "tcp://0.0.0.0:8000".parse().unwrap(),
///     ],
///     ..Config::default()
/// };
///
/// let node = Node::new(&key_pair, config, global_executor());
///
/// // node.run().await.unwrap();
/// // node.attach_protocol(MyProto::new).await;
/// // node.shutdown().await;
/// ```
pub struct Node {
    /// The Configuration for the P2P network.
    config: Arc<Config>,

    /// Identity Key pair
    key_pair: KeyPair,

    /// Peer ID
    peer_id: PeerID,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,

    /// Discovery instance.
    discovery: Arc<dyn Discovery>,

    /// PeerPool instance.
    peer_pool: Arc<PeerPool>,

    /// Connector for outbound connections.
    connector: Arc<Connector<PeerNetMsgCodec>>,

    /// Listener for inbound connections.
    listener: Arc<Listener<PeerNetMsgCodec>>,

    /// Managing spawned tasks.
    task_group: TaskGroup,
}

impl Node {
    /// Creates a new Node that dials `Config::dial_peers` and finds no
    /// other peers. Use `with_discovery` to run a discovery.
    pub fn new(key_pair: &KeyPair, config: Config, ex: Executor) -> Arc<Node> {
        Self::with_discovery(
            key_pair,
            config,
            |ctx| StaticDiscovery::new(&ctx.config),
            ex,
        )
    }

    /// Creates a new Node with a custom discovery. `discovery` builds it
    /// from what the node shares with it: key pair, PeerID, config,
    /// monitor and executor.
    pub fn with_discovery(
        key_pair: &KeyPair,
        config: Config,
        discovery: impl FnOnce(&DiscoveryContext) -> Arc<dyn Discovery>,
        ex: Executor,
    ) -> Arc<Node> {
        let config = Arc::new(config);
        let monitor = Arc::new(Monitor::new(config.clone()));
        let peer_id = PeerID::try_from(key_pair.public())
            .expect("Derive a peer id from the provided key pair.");
        info!("PeerID: {peer_id}");

        let conn_queue = ConnQueue::new();
        let peer_pool = PeerPool::new(
            &peer_id,
            conn_queue.clone(),
            config.clone(),
            monitor.clone(),
            ex.clone(),
        );

        let discovery = discovery(&DiscoveryContext {
            key_pair: key_pair.clone(),
            peer_id: peer_id.clone(),
            config: config.clone(),
            monitor: monitor.clone(),
            executor: ex.clone(),
        });

        let connector = Connector::new_with_queue(
            key_pair,
            config.max_connect_retries,
            config.outbound_slots,
            config.handshake_timeout,
            conn_queue.clone(),
            monitor.clone(),
            ex.clone(),
        );

        let listener = Listener::new_with_queue(
            key_pair,
            config.inbound_slots,
            conn_queue,
            monitor.clone(),
            config.handshake_timeout,
            config.access_control.clone(),
            ex.clone(),
        );

        let task_group = TaskGroup::with_executor(ex);

        Arc::new(Self {
            key_pair: key_pair.clone(),
            peer_id,
            monitor,
            discovery,
            config,
            peer_pool,
            connector,
            listener,
            task_group,
        })
    }

    /// Run the Node, starting listeners, PeerPool, and Discovery.
    pub async fn run(self: &Arc<Self>) -> Result<()> {
        // Core protocols (PING) are attached before the pool starts so
        // they're advertised on every handshake.
        self.attach_core_protocols().await?;

        self.peer_pool.start().await?;

        // Start data listeners.
        for endpoint in &self.config.listen_endpoints {
            let resolved = self.listener.start(endpoint.clone()).await?;
            info!("Listening on {resolved}");
        }

        // Start discovery.
        self.discovery.clone().start().await?;

        // Forward peer lifecycle events to discovery.
        self.task_group.spawn_then(
            {
                let this = self.clone();
                async move { this.forward_peer_events().await }
            },
            |res: TaskResult<()>| async move {
                debug!("forward_peer_events task ended: {res}");
            },
        );

        // Spawn task to connect discovered peers.
        self.task_group.spawn_then(
            {
                let this = self.clone();
                async move { this.connect_discovered_peers().await }
            },
            |res: TaskResult<()>| async move {
                debug!("connect_discovered_peers task ended: {res}");
            },
        );

        Ok(())
    }

    /// Forward peer-pool lifecycle events to discovery so it can
    /// update its routing state. Each registered listener (Node's
    /// here, plus any external Swarm subscriber) gets every event
    /// independently - no MPMC stealing.
    async fn forward_peer_events(self: Arc<Self>) {
        let listener = self.peer_pool.register_peer_events();
        while let Ok(event) = listener.recv().await {
            let mapped = match event {
                PeerEvent::Added(pid) => PeerConnectionEvent::Connected(pid),
                PeerEvent::Removed(pid) => PeerConnectionEvent::Disconnected(pid),
                PeerEvent::HandshakeFailed(pid) => PeerConnectionEvent::ConnectFailed(pid),
            };
            self.discovery.on_event(mapped);
        }
    }

    /// Consume discovered peers from the discovery service and connect to them.
    /// Runs forever; the task_group cancels it on Node::shutdown.
    async fn connect_discovered_peers(self: Arc<Self>) {
        let supported = [Protocol::Tcp, Protocol::Tls, Protocol::Quic];

        loop {
            let discovered = self.discovery.recv().await;

            let endpoint = match pick_endpoint(&discovered.addrs, &supported) {
                Some(ep) => ep,
                None => continue,
            };

            if !self.config.access_control.allow(
                &Subject::Endpoint(&endpoint),
                Action::Connect(ConnDirection::Outbound),
            ) {
                debug!("Skipped dialing denied endpoint {endpoint}");
                continue;
            }

            // Already connected: a second connection would only be
            // rejected or replace the first.
            if let Some(pid) = &discovered.peer_id
                && self.peer_pool.contains_peer(pid).await
            {
                continue;
            }

            // Take the slot here, so this loop waits when all slots are
            // busy. The dial runs in its own task, so one slow endpoint
            // does not hold back the next dial.
            let slot = match self.connector.acquire_slot().await {
                Ok(slot) => slot,
                Err(err) => {
                    error!("Acquire outbound slot: {err}");
                    break;
                }
            };

            let peer_id = discovered.peer_id.clone();
            let this = self.clone();
            self.task_group.spawn(async move {
                let res = this
                    .connector
                    .connect_and_queue(&endpoint, &peer_id, slot)
                    .await;
                if res.is_err() {
                    this.monitor
                        .notify(PoolEvent::ConnectFailed(peer_id.clone(), endpoint))
                        .await;
                    this.discovery
                        .on_event(PeerConnectionEvent::ConnectFailed(peer_id));
                }
            });
        }
    }

    /// Attach a custom protocol. karyon runs the constructor closure
    /// once per connected peer with a typed `PeerConn` scoped to this
    /// protocol. The protocol type is inferred from the closure's
    /// return value. The protocol id is advertised through discovery
    /// according to `P::flags()`.
    ///
    /// ```ignore
    /// node.attach_protocol(MyProtocol::new).await?;
    /// ```
    pub async fn attach_protocol<P: ProtocolTrait + 'static>(
        &self,
        c: impl Fn(PeerConn) -> P + Send + Sync + 'static,
    ) -> Result<()> {
        let c = move |conn| Arc::new(c(conn)) as Arc<dyn ProtocolTrait>;
        self.peer_pool.attach_protocol::<P>(Box::new(c)).await?;
        self.advertise(P::id().as_bytes(), P::flags());
        Ok(())
    }

    /// Attach the core protocols (PING). Called once during `run`.
    async fn attach_core_protocols(self: &Arc<Self>) -> Result<()> {
        self.attach_protocol(PingProtocol::new).await
    }

    /// Advertise an item (protocol id, swarm key, ...) through
    /// discovery. See [`ProtocolFlags`]. Used by Swarm and other
    /// layers.
    pub fn advertise(&self, item: impl AsRef<[u8]>, flags: ProtocolFlags) {
        self.discovery.advertise(item.as_ref(), flags);
    }

    /// Find peers known to the discovery that may support `item`.
    /// Useful for swarm-targeted lookups (e.g. "peers in this room")
    /// without changing handshake semantics.
    pub fn find_peers_with(&self, item: impl AsRef<[u8]>) -> Vec<DiscoveredPeer> {
        self.discovery.find_peers_with(item.as_ref())
    }

    /// Returns the number of currently connected peers.
    pub async fn peers(&self) -> usize {
        self.peer_pool.peers_len().await
    }

    /// Returns the `Config`.
    pub fn config(&self) -> Arc<Config> {
        self.config.clone()
    }

    /// Returns the `PeerID`.
    pub fn peer_id(&self) -> &PeerID {
        &self.peer_id
    }

    /// Returns the `KeyPair`.
    pub fn key_pair(&self) -> &KeyPair {
        &self.key_pair
    }

    /// Returns a map of inbound connected peers with their endpoints.
    pub async fn inbound_peers(&self) -> HashMap<PeerID, Endpoint> {
        self.peer_pool.inbound_peers().await
    }

    /// Returns a map of outbound connected peers with their endpoints.
    pub async fn outbound_peers(&self) -> HashMap<PeerID, Endpoint> {
        self.peer_pool.outbound_peers().await
    }

    /// Returns the monitor to receive system events.
    pub fn monitor(&self) -> Arc<Monitor> {
        self.monitor.clone()
    }

    /// Register a listener for peer lifecycle events. Each call returns
    /// a fresh listener that receives every event (true broadcast).
    pub fn register_peer_events(&self) -> EventListener<PeerEventTopic, PeerEvent> {
        self.peer_pool.register_peer_events()
    }

    /// Broadcast a message to a specific set of peers on a given protocol.
    /// Used by Swarm and other layers to scope broadcasts.
    pub async fn broadcast_to(
        &self,
        proto_id: &ProtocolID,
        msg: Vec<u8>,
        targets: &HashSet<PeerID>,
    ) {
        self.peer_pool.broadcast_to(proto_id, msg, targets).await;
    }

    /// Send a message to a specific peer on the given protocol.
    /// Returns `PeerNotFound` if the peer is not currently connected.
    pub async fn send_to(
        &self,
        peer_id: &PeerID,
        proto_id: &ProtocolID,
        msg: Vec<u8>,
    ) -> Result<()> {
        self.peer_pool.send_to(peer_id, proto_id, msg).await
    }

    /// Returns the negotiated protocol set for a connected peer, or
    /// `None` if no peer with that id is currently in the pool.
    pub async fn peer_protocol_set(&self, pid: &PeerID) -> Option<HashSet<ProtocolID>> {
        self.peer_pool.peer_protocol_set(pid).await
    }

    /// Shuts down the Node.
    pub async fn shutdown(&self) {
        self.discovery.shutdown().await;
        self.peer_pool.shutdown().await;
        self.connector.shutdown().await;
        self.listener.shutdown().await;
        self.task_group.cancel().await;
    }
}
