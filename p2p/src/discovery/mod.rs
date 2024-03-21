mod lookup;
mod refresh;

use std::sync::Arc;

use log::{error, info};
use rand::{rngs::OsRng, seq::SliceRandom};
use smol::lock::Mutex;

use karyon_core::{
    async_util::{Backoff, Executor, TaskGroup, TaskResult},
    crypto::KeyPair,
};

use karyon_net::{Conn, Endpoint};

use crate::{
    config::Config,
    connection::{ConnDirection, ConnQueue},
    connector::Connector,
    listener::Listener,
    monitor::Monitor,
    routing_table::{
        Entry, EntryStatusFlag, RoutingTable, CONNECTED_ENTRY, DISCONNECTED_ENTRY,
        INCOMPATIBLE_ENTRY, PENDING_ENTRY, UNREACHABLE_ENTRY, UNSTABLE_ENTRY,
    },
    slots::ConnectionSlots,
    Error, PeerID, Result,
};

use lookup::LookupService;
use refresh::RefreshService;

pub type ArcDiscovery = Arc<Discovery>;

pub struct Discovery {
    /// Routing table
    table: Arc<Mutex<RoutingTable>>,

    /// Lookup Service
    lookup_service: Arc<LookupService>,

    /// Refresh Service
    refresh_service: Arc<RefreshService>,

    /// Connector
    connector: Arc<Connector>,
    /// Listener
    listener: Arc<Listener>,

    /// Connection queue
    conn_queue: Arc<ConnQueue>,

    /// Inbound slots.
    pub(crate) inbound_slots: Arc<ConnectionSlots>,
    /// Outbound slots.
    pub(crate) outbound_slots: Arc<ConnectionSlots>,

    /// Managing spawned tasks.
    task_group: TaskGroup<'static>,

    /// Holds the configuration for the P2P network.
    config: Arc<Config>,
}

impl Discovery {
    /// Creates a new Discovery
    pub fn new(
        key_pair: &KeyPair,
        peer_id: &PeerID,
        conn_queue: Arc<ConnQueue>,
        config: Arc<Config>,
        monitor: Arc<Monitor>,
        ex: Executor<'static>,
    ) -> ArcDiscovery {
        let inbound_slots = Arc::new(ConnectionSlots::new(config.inbound_slots));
        let outbound_slots = Arc::new(ConnectionSlots::new(config.outbound_slots));

        let table_key = peer_id.0;
        let table = Arc::new(Mutex::new(RoutingTable::new(table_key)));

        let refresh_service =
            RefreshService::new(config.clone(), table.clone(), monitor.clone(), ex.clone());
        let lookup_service = LookupService::new(
            key_pair,
            peer_id,
            table.clone(),
            config.clone(),
            monitor.clone(),
            ex.clone(),
        );

        let connector = Connector::new(
            key_pair,
            config.max_connect_retries,
            outbound_slots.clone(),
            config.enable_tls,
            monitor.clone(),
            ex.clone(),
        );

        let listener = Listener::new(
            key_pair,
            inbound_slots.clone(),
            config.enable_tls,
            monitor.clone(),
            ex.clone(),
        );

        Arc::new(Self {
            refresh_service: Arc::new(refresh_service),
            lookup_service: Arc::new(lookup_service),
            conn_queue,
            table,
            inbound_slots,
            outbound_slots,
            connector,
            listener,
            task_group: TaskGroup::with_executor(ex),
            config,
        })
    }

    /// Start the Discovery
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        // Check if the listen_endpoint is provided, and if so, start a listener.
        if let Some(endpoint) = &self.config.listen_endpoint {
            // Return an error if the discovery port is set to 0.
            if self.config.discovery_port == 0 {
                return Err(Error::Config(
                    "Please add a valid discovery port".to_string(),
                ));
            }

            let resolved_endpoint = self.start_listener(endpoint).await?;

            if endpoint.addr()? != resolved_endpoint.addr()? {
                info!("Resolved listen endpoint: {resolved_endpoint}");
                self.lookup_service
                    .set_listen_endpoint(&resolved_endpoint)
                    .await;
                self.refresh_service
                    .set_listen_endpoint(&resolved_endpoint)
                    .await;
            }
        }

        // Start the lookup service
        self.lookup_service.start().await?;
        // Start the refresh service
        self.refresh_service.start().await?;

        // Attempt to manually connect to peer endpoints provided in the Config.
        for endpoint in self.config.peer_endpoints.iter() {
            let _ = self.connect(endpoint, None).await;
        }

        // Start connect loop
        let selfc = self.clone();
        self.task_group
            .spawn(selfc.connect_loop(), |res| async move {
                if let TaskResult::Completed(Err(err)) = res {
                    error!("Connect loop stopped: {err}");
                }
            });

        Ok(())
    }

    /// Shuts down the discovery
    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
        self.connector.shutdown().await;
        self.listener.shutdown().await;

        self.refresh_service.shutdown().await;
        self.lookup_service.shutdown().await;
    }

    /// Start a listener and on success, return the resolved endpoint.
    async fn start_listener(self: &Arc<Self>, endpoint: &Endpoint) -> Result<Endpoint> {
        let selfc = self.clone();
        let callback = |c: Conn| async move {
            selfc.conn_queue.handle(c, ConnDirection::Inbound).await?;
            Ok(())
        };

        let resolved_endpoint = self.listener.start(endpoint.clone(), callback).await?;
        Ok(resolved_endpoint)
    }

    /// This method will attempt to connect to a peer in the routing table.
    /// If the routing table is empty, it will start the seeding process for
    /// finding new peers.
    ///
    /// This will perform a backoff to prevent getting stuck in the loop
    /// if the seeding process couldn't find any peers.
    async fn connect_loop(self: Arc<Self>) -> Result<()> {
        let backoff = Backoff::new(500, self.config.seeding_interval * 1000);
        loop {
            let random_entry = self.random_entry(PENDING_ENTRY).await;
            match random_entry {
                Some(entry) => {
                    backoff.reset();
                    let endpoint = Endpoint::Tcp(entry.addr, entry.port);
                    self.connect(&endpoint, Some(entry.key.into())).await;
                }
                None => {
                    backoff.sleep().await;
                    self.start_seeding().await;
                }
            }
        }
    }

    /// Connect to the given endpoint using the connector
    async fn connect(self: &Arc<Self>, endpoint: &Endpoint, pid: Option<PeerID>) {
        let selfc = self.clone();
        let pid_c = pid.clone();
        let endpoint_c = endpoint.clone();
        let cback = |conn: Conn| async move {
            let result = selfc.conn_queue.handle(conn, ConnDirection::Outbound).await;

            // If the entry is not in the routing table, ignore the result
            let pid = match pid_c {
                Some(p) => p,
                None => return Ok(()),
            };

            match result {
                Err(Error::IncompatiblePeer) => {
                    error!("Failed to do handshake: {endpoint_c} incompatible peer");
                    selfc.update_entry(&pid, INCOMPATIBLE_ENTRY).await;
                }
                Err(Error::PeerAlreadyConnected) => {
                    // TODO: Use the appropriate status.
                    selfc.update_entry(&pid, DISCONNECTED_ENTRY).await;
                }
                Err(_) => {
                    selfc.update_entry(&pid, UNSTABLE_ENTRY).await;
                }
                Ok(_) => {
                    selfc.update_entry(&pid, DISCONNECTED_ENTRY).await;
                }
            }

            Ok(())
        };

        let result = self
            .connector
            .connect_with_cback(endpoint, &pid, cback)
            .await;

        if let Some(pid) = &pid {
            match result {
                Ok(_) => {
                    self.update_entry(pid, CONNECTED_ENTRY).await;
                }
                Err(_) => {
                    self.update_entry(pid, UNREACHABLE_ENTRY).await;
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
        match self.random_entry(PENDING_ENTRY | CONNECTED_ENTRY).await {
            Some(entry) => {
                let endpoint = Endpoint::Tcp(entry.addr, entry.discovery_port);
                let peer_id = Some(entry.key.into());
                if let Err(err) = self.lookup_service.start_lookup(&endpoint, peer_id).await {
                    self.update_entry(&entry.key.into(), UNSTABLE_ENTRY).await;
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

    /// Returns a random entry from routing table.
    async fn random_entry(&self, entry_flag: EntryStatusFlag) -> Option<Entry> {
        self.table.lock().await.random_entry(entry_flag).cloned()
    }

    /// Update the entry status  
    async fn update_entry(&self, pid: &PeerID, entry_flag: EntryStatusFlag) {
        let table = &mut self.table.lock().await;
        table.update_entry(&pid.0, entry_flag);
    }
}
