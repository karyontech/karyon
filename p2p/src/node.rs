use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use log::{debug, error, info};

use karyon_core::{
    async_runtime::Executor,
    async_util::{AsyncQueue, TaskGroup, TaskResult},
    crypto::KeyPair,
};
use karyon_net::Endpoint;

use crate::{
    PeerID, Result,
    access_control::{Action, Subject},
    admission::AdmitConfig,
    codec::PeerNetMsgCodec,
    config::{Config, ConfigRef},
    conn_direction::ConnDirection,
    connector::{Connector, ConnectorRef},
    discovery::{DiscoveredPeer, DiscoveryContext, DiscoveryRef, NoDiscovery, PeerConnectionEvent},
    listener::{Listener, ListenerRef},
    message::{PeerAddr, Protocol, pick_endpoint},
    monitor::{Monitor, MonitorRef, PoolEvent},
    peer_events::{PeerEventListener, PeerEvents, PeerEventsRef},
    peer_pool::{PeerPool, PeerPoolRef},
    protocol::{
        PeerConn, Protocol as ProtocolTrait, ProtocolFlags, ProtocolID, ProtocolMeta, ProtocolRef,
        ProtocolRegistry, ProtocolRegistryRef,
    },
    protocols::PingProtocol,
    queued_conn::CONN_QUEUE_SIZE,
};

/// `Arc` reference to a `Node`.
pub type NodeRef = Arc<Node>;

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
    config: ConfigRef,

    /// Identity Key pair
    key_pair: KeyPair,

    /// Peer ID
    peer_id: PeerID,

    /// Responsible for network and system monitoring.
    monitor: MonitorRef,

    /// Discovery instance.
    discovery: DiscoveryRef,

    /// Attached protocols, shared with the PeerPool.
    registry: ProtocolRegistryRef,

    /// Owns the sessions and runs their lifecycle.
    peer_pool: PeerPoolRef,

    /// Where the PeerPool reports peer events.
    peer_events: PeerEventsRef,

    /// Connector for outbound connections.
    connector: ConnectorRef<PeerNetMsgCodec>,

    /// Listener for inbound connections.
    listener: ListenerRef<PeerNetMsgCodec>,

    /// Managing spawned tasks.
    task_group: TaskGroup,
}

impl Node {
    /// Creates a new Node that runs no discovery: it only dials
    /// `Config::dial_peers`. Use `with_discovery` to find more peers.
    pub fn new(key_pair: &KeyPair, config: Config, ex: Executor) -> NodeRef {
        Self::with_discovery(key_pair, config, |_| Arc::new(NoDiscovery), ex)
    }

    /// Creates a new Node with a custom discovery. `discovery` builds it
    /// from what the node shares with it: key pair, PeerID, config,
    /// monitor and executor.
    pub fn with_discovery(
        key_pair: &KeyPair,
        config: Config,
        discovery: impl FnOnce(&DiscoveryContext) -> DiscoveryRef,
        ex: Executor,
    ) -> NodeRef {
        let config = Arc::new(config);
        let monitor = Monitor::new(config.enable_monitor);
        let peer_id = PeerID::try_from(key_pair.public())
            .expect("Derive a peer id from the provided key pair.");
        info!("PeerID: {peer_id}");

        let discovery = discovery(&DiscoveryContext {
            key_pair: key_pair.clone(),
            peer_id: peer_id.clone(),
            config: config.clone(),
            monitor: monitor.clone(),
            executor: ex.clone(),
        });

        // The listener and the connector hand their connections to the
        // PeerPool over this queue.
        let conn_queue = AsyncQueue::new(CONN_QUEUE_SIZE);
        let registry = ProtocolRegistry::new();
        let peer_events = PeerEvents::new(monitor.clone(), discovery.clone());
        let admit_config = AdmitConfig {
            version: config.version.clone(),
            handshake_timeout: config.handshake_timeout,
            access_control: config.access_control.clone(),
        };
        let peer_pool = PeerPool::new(
            &peer_id,
            conn_queue.clone(),
            registry.clone(),
            admit_config,
            peer_events.clone(),
            ex.clone(),
        );

        let connector = Connector::new_with_queue(
            key_pair,
            &config,
            conn_queue.clone(),
            monitor.clone(),
            ex.clone(),
        );

        let listener =
            Listener::new_with_queue(key_pair, &config, conn_queue, monitor.clone(), ex.clone());

        let task_group = TaskGroup::with_executor(ex);

        Arc::new(Self {
            key_pair: key_pair.clone(),
            peer_id,
            monitor,
            discovery,
            config,
            registry,
            peer_pool,
            peer_events,
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

    /// Dial `Config::dial_peers` once, then the peers the discovery
    /// yields. Runs forever; the task_group cancels it on Node::shutdown.
    async fn connect_discovered_peers(self: Arc<Self>) {
        for endpoint in self.config.dial_peers.iter() {
            let peer = DiscoveredPeer {
                peer_id: None,
                addrs: PeerAddr::from_endpoint(endpoint, 0).into_iter().collect(),
            };
            if self.dial(peer).await.is_err() {
                return;
            }
        }

        loop {
            let peer = self.discovery.recv().await;
            if self.dial(peer).await.is_err() {
                return;
            }
        }
    }

    /// Dial `peer` in its own task once an outbound slot is free. Skips
    /// peers with no usable address, denied or already connected. Fails
    /// only if the outbound slots are closed.
    async fn dial(self: &Arc<Self>, peer: DiscoveredPeer) -> Result<()> {
        let supported = [Protocol::Tcp, Protocol::Tls, Protocol::Quic];
        let Some(endpoint) = pick_endpoint(&peer.addrs, &supported, self.config.transport.as_ref())
        else {
            return Ok(());
        };

        if !self.config.access_control.allow(
            &Subject::Endpoint(&endpoint),
            Action::Connect(ConnDirection::Outbound),
        ) {
            debug!("Skipped dialing denied endpoint {endpoint}");
            return Ok(());
        }

        // Already connected: a second connection would only be
        // rejected or replace the first.
        if let Some(pid) = &peer.peer_id
            && self.peer_pool.contains_peer(pid).await
        {
            return Ok(());
        }

        // Take the slot here, so the caller waits when all slots are
        // busy. The dial runs in its own task, so one slow endpoint
        // does not hold back the next dial.
        let slot = self
            .connector
            .acquire_slot()
            .await
            .inspect_err(|err| error!("Acquire outbound slot: {err}"))?;

        let peer_id = peer.peer_id;
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
                    .on_event(PeerConnectionEvent::DialFailed(peer_id));
            }
        });
        Ok(())
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
        let meta = ProtocolMeta {
            version: P::version()?,
            flags: P::flags(),
        };
        let c = move |conn| Arc::new(c(conn)) as ProtocolRef;
        self.registry.insert(P::id(), Arc::new(c), meta).await;
        self.advertise(P::id().as_bytes(), P::flags());
        Ok(())
    }

    /// Attach the core protocols (PING). Called once during `run`.
    async fn attach_core_protocols(&self) -> Result<()> {
        let interval = self.config.ping_interval;
        let timeout = self.config.ping_timeout;
        let ex = self.task_group.executor().clone();
        self.attach_protocol(move |peer| PingProtocol::new(peer, interval, timeout, ex.clone()))
            .await
    }

    /// Advertise an item (protocol id, swarm key, ...) through
    /// discovery. See [`ProtocolFlags`]. Used by Swarm and other
    /// layers.
    pub fn advertise(&self, item: impl AsRef<[u8]>, flags: ProtocolFlags) {
        self.discovery.advertise(item.as_ref(), flags);
    }

    /// Returns the number of currently connected peers.
    pub async fn peers(&self) -> usize {
        self.peer_pool.peers_len().await
    }

    /// Returns the `Config`.
    pub fn config(&self) -> ConfigRef {
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
    pub fn monitor(&self) -> MonitorRef {
        self.monitor.clone()
    }

    /// Register a listener for peer lifecycle events. Each call returns
    /// a fresh listener that receives every event (true broadcast).
    pub fn register_peer_events(&self) -> PeerEventListener {
        self.peer_events.register()
    }

    /// Broadcast a message to every connected peer on a given protocol.
    pub async fn broadcast(&self, proto_id: &ProtocolID, msg: Vec<u8>) {
        self.peer_pool.broadcast(proto_id, msg).await;
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
        // Stop everything that hands connections to the PeerPool first
        // (the dial tasks run in this task group), so the pool can drop
        // the rest of its queue.
        self.listener.shutdown().await;
        self.connector.shutdown().await;
        self.task_group.cancel().await;
        self.peer_pool.shutdown().await;
    }
}
