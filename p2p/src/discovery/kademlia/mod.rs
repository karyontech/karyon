mod lookup;
mod messages;
mod refresh;
pub mod routing_table;

use std::sync::Arc;

use async_trait::async_trait;
use log::error;
use rand::{rngs::OsRng, seq::SliceRandom};

use karyon_core::{
    async_runtime::Executor,
    async_util::{AsyncQueue, Backoff, TaskGroup, TaskResult},
    crypto::KeyPair,
};

use karyon_net::Endpoint;

use crate::{
    bloom::BloomRef,
    config::Config,
    discovery::{DiscoveredPeer, Discovery, PeerConnectionEvent},
    message::{pick_endpoint, PeerAddr, Protocol},
    monitor::Monitor,
    PeerID, Result,
};

use lookup::{LookupEndpoints, LookupService};
use refresh::RefreshService;
use routing_table::{
    RoutingTable, CONNECTED_ENTRY, DISCONNECTED_ENTRY, PENDING_ENTRY, UNREACHABLE_ENTRY,
    UNSTABLE_ENTRY,
};

/// Bound on the discovered-peer queue between Kademlia and the Node.
const DISCOVERED_PEER_QUEUE_SIZE: usize = 128;

/// Transports the lookup service can dial. Computed once here so the
/// rest of the module never needs an inline cfg.
#[cfg(feature = "quic")]
pub(crate) const SUPPORTED_LOOKUP_PROTOCOLS: &[Protocol] = &[Protocol::Tcp, Protocol::Quic];
#[cfg(not(feature = "quic"))]
pub(crate) const SUPPORTED_LOOKUP_PROTOCOLS: &[Protocol] = &[Protocol::Tcp];

pub struct KademliaDiscovery {
    /// Routing table
    table: Arc<RoutingTable>,

    /// Lookup Service
    lookup_service: Arc<LookupService>,

    /// Refresh Service
    refresh_service: Arc<RefreshService>,

    /// Discovered peers queued for the Node to dial. Producers
    /// (connect_loop, manual peer_endpoints) push; the Node's
    /// connect_discovered_peers task drains via `recv()`.
    peer_queue: Arc<AsyncQueue<DiscoveredPeer>>,

    /// Managing spawned tasks.
    task_group: TaskGroup,

    /// Holds the configuration for the P2P network.
    config: Arc<Config>,

    /// Shared local bloom. Snapshotted on every connect-loop iteration
    /// to filter routing-table entries (and stamped on outgoing PeerMsgs
    /// via the lookup service).
    bloom: BloomRef,
}

impl KademliaDiscovery {
    /// Creates a new KademliaDiscovery
    pub fn new(
        key_pair: &KeyPair,
        peer_id: &PeerID,
        config: Arc<Config>,
        monitor: Arc<Monitor>,
        bloom: BloomRef,
        ex: Executor,
    ) -> Arc<Self> {
        let table = Arc::new(RoutingTable::new(peer_id.0));

        // Pick the lookup endpoint (tcp/tls/quic) and the refresh
        // endpoint (udp) from `discovery_endpoints` (any order).
        let lookup_ep = config
            .discovery_endpoints
            .iter()
            .find(|e| is_lookup_proto(e))
            .cloned();
        let refresh_ep = config
            .discovery_endpoints
            .iter()
            .find(|e| e.is_udp())
            .cloned();

        let refresh_service = Arc::new(RefreshService::new(
            config.clone(),
            table.clone(),
            monitor.clone(),
            refresh_ep.clone(),
            ex.clone(),
        ));

        let lookup_endpoints = LookupEndpoints {
            listen: config.listen_endpoints.clone(),
            lookup: lookup_ep,
            refresh: refresh_ep,
        };
        let lookup_service = Arc::new(LookupService::new(
            key_pair,
            table.clone(),
            config.clone(),
            monitor.clone(),
            bloom.clone(),
            lookup_endpoints,
            ex.clone(),
        ));

        let task_group = TaskGroup::with_executor(ex);

        let peer_queue = AsyncQueue::new(DISCOVERED_PEER_QUEUE_SIZE);

        Arc::new(Self {
            refresh_service,
            lookup_service,
            table,
            peer_queue,
            task_group,
            config,
            bloom,
        })
    }

    /// This method will attempt to find a peer in the routing table and
    /// send it as a DiscoveredPeer for the Node to connect to.
    /// If the routing table is empty, it will start the seeding process.
    async fn connect_loop(self: Arc<Self>) -> Result<()> {
        let backoff = Backoff::new(500, self.config.seeding_interval * 1000);
        loop {
            let required = *self.bloom.read();
            match self.table.random_entry_filtered(PENDING_ENTRY, &required) {
                Some(entry) => {
                    backoff.reset();
                    let key = entry.key;
                    let peer = DiscoveredPeer {
                        peer_id: Some(key.into()),
                        addrs: entry.addrs.clone(),
                        discovery_addrs: entry.discovery_addrs.clone(),
                    };
                    // Mark as connected to prevent re-picking while
                    // the Node is still connecting.
                    self.table.update_entry(&key, CONNECTED_ENTRY);
                    self.peer_queue.push(peer).await;
                }
                None => {
                    backoff.sleep().await;
                    self.start_seeding().await;
                }
            }
        }
    }

    /// Starts seeding process.
    ///
    /// This method randomly selects a peer from the routing table and
    /// attempts to connect to that peer for the initial lookup. If the routing
    /// table doesn't have an available entry, it will connect to one of the
    /// provided bootstrap endpoints in the `Config` and initiate the lookup.
    async fn start_seeding(&self) {
        match self.table.random_entry(PENDING_ENTRY | CONNECTED_ENTRY) {
            Some(entry) => {
                let endpoint =
                    match pick_endpoint(&entry.discovery_addrs, SUPPORTED_LOOKUP_PROTOCOLS) {
                        Some(ep) => ep,
                        None => return,
                    };
                let peer_id = Some(entry.key.into());
                if let Err(err) = self.lookup_service.start_lookup(&endpoint, peer_id).await {
                    self.table.update_entry(&entry.key, UNSTABLE_ENTRY);
                    error!("Failed to do lookup: {endpoint}: {err}");
                }
            }
            None => {
                let peers = &self.config.bootstrap_peers;
                for endpoint in peers.choose_multiple(&mut OsRng, peers.len()) {
                    if let Err(err) = self.lookup_service.start_lookup(endpoint, None).await {
                        error!("Failed to do lookup: {endpoint}: {err}");
                    }
                }
            }
        }
    }
}

#[async_trait]
impl Discovery for KademliaDiscovery {
    async fn start(self: Arc<Self>) -> Result<()> {
        // Start the lookup service
        self.lookup_service.start().await?;
        // Start the refresh service
        self.refresh_service.start().await?;

        // Send manual peer endpoints as DiscoveredPeer
        for endpoint in self.config.peer_endpoints.iter() {
            if let Some(addr) = PeerAddr::from_endpoint(endpoint, 0) {
                let peer = DiscoveredPeer {
                    peer_id: None,
                    addrs: vec![addr],
                    discovery_addrs: vec![],
                };
                self.peer_queue.push(peer).await;
            }
        }

        // Start connect loop
        self.task_group.spawn(
            {
                let this = self.clone();
                async move { this.connect_loop().await }
            },
            |res| async move {
                if let TaskResult::Completed(Err(err)) = res {
                    error!("Connect loop stopped: {err}");
                }
            },
        );

        Ok(())
    }

    async fn shutdown(&self) {
        self.task_group.cancel().await;
        self.refresh_service.shutdown().await;
        self.lookup_service.shutdown().await;
    }

    async fn recv(&self) -> DiscoveredPeer {
        self.peer_queue.recv().await
    }

    fn on_event(&self, event: PeerConnectionEvent) {
        match event {
            PeerConnectionEvent::Connected(pid) => {
                self.table.update_entry(&pid.0, CONNECTED_ENTRY);
            }
            PeerConnectionEvent::Disconnected(pid) => {
                self.table.update_entry(&pid.0, DISCONNECTED_ENTRY);
            }
            PeerConnectionEvent::ConnectFailed(Some(pid)) => {
                self.table.update_entry(&pid.0, UNREACHABLE_ENTRY);
            }
            PeerConnectionEvent::ConnectFailed(None) => {}
        }
    }

    fn find_peers_with(&self, item: &[u8]) -> Vec<DiscoveredPeer> {
        self.table
            .entries_with_item(item)
            .into_iter()
            .map(|e| DiscoveredPeer {
                peer_id: Some(e.key.into()),
                addrs: e.addrs,
                discovery_addrs: e.discovery_addrs,
            })
            .collect()
    }
}

/// Returns true if the endpoint can serve the Kademlia lookup service.
fn is_lookup_proto(e: &Endpoint) -> bool {
    if e.is_tcp() {
        return true;
    }
    #[cfg(feature = "quic")]
    if e.is_quic() {
        return true;
    }
    false
}
