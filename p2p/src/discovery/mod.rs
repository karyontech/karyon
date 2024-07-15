mod lookup;
mod refresh;

use std::sync::Arc;

use log::{error, info};
use rand::{rngs::OsRng, seq::SliceRandom};

use karyon_core::{
    async_runtime::Executor,
    async_util::{Backoff, TaskGroup, TaskResult},
    crypto::KeyPair,
};

use karyon_net::{Conn, Endpoint};

use crate::{
    config::Config,
    conn_queue::ConnQueue,
    connection::ConnDirection,
    connector::Connector,
    listener::Listener,
    message::NetMsg,
    monitor::Monitor,
    routing_table::{
        RoutingTable, CONNECTED_ENTRY, DISCONNECTED_ENTRY, INCOMPATIBLE_ENTRY, PENDING_ENTRY,
        UNREACHABLE_ENTRY, UNSTABLE_ENTRY,
    },
    slots::ConnectionSlots,
    Error, PeerID, Result,
};

use lookup::LookupService;
use refresh::RefreshService;

pub struct Discovery {
    /// Routing table
    table: Arc<RoutingTable>,

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

    /// Managing spawned tasks.
    task_group: TaskGroup,

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
        ex: Executor,
    ) -> Arc<Discovery> {
        let table = Arc::new(RoutingTable::new(peer_id.0));

        let refresh_service = Arc::new(RefreshService::new(
            config.clone(),
            table.clone(),
            monitor.clone(),
            ex.clone(),
        ));

        let lookup_service = Arc::new(LookupService::new(
            key_pair,
            peer_id,
            table.clone(),
            config.clone(),
            monitor.clone(),
            ex.clone(),
        ));

        let outbound_slots = Arc::new(ConnectionSlots::new(config.outbound_slots));
        let connector = Connector::new(
            key_pair,
            config.max_connect_retries,
            outbound_slots.clone(),
            config.enable_tls,
            monitor.clone(),
            ex.clone(),
        );

        let inbound_slots = Arc::new(ConnectionSlots::new(config.inbound_slots));
        let listener = Listener::new(
            key_pair,
            inbound_slots.clone(),
            config.enable_tls,
            monitor.clone(),
            ex.clone(),
        );

        let task_group = TaskGroup::with_executor(ex);

        Arc::new(Self {
            refresh_service,
            lookup_service,
            conn_queue,
            table,
            connector,
            listener,
            task_group,
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

            info!("Resolved listen endpoint: {resolved_endpoint}");
            self.lookup_service
                .set_listen_endpoint(&resolved_endpoint)?;
            self.refresh_service
                .set_listen_endpoint(&resolved_endpoint)?;
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
        let callback = {
            let this = self.clone();
            |c: Conn<NetMsg>| async move {
                this.conn_queue.handle(c, ConnDirection::Inbound).await?;
                Ok(())
            }
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
            match self.table.random_entry(PENDING_ENTRY) {
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
        let cback = {
            let this = self.clone();
            let endpoint = endpoint.clone();
            let pid = pid.clone();
            |conn: Conn<NetMsg>| async move {
                let result = this.conn_queue.handle(conn, ConnDirection::Outbound).await;

                // If the entry is not in the routing table, ignore the result
                let pid = match pid {
                    Some(p) => p,
                    None => return Ok(()),
                };

                match result {
                    Err(Error::IncompatiblePeer) => {
                        error!("Failed to do handshake: {endpoint} incompatible peer");
                        this.table.update_entry(&pid.0, INCOMPATIBLE_ENTRY);
                    }
                    Err(Error::PeerAlreadyConnected) => {
                        this.table.update_entry(&pid.0, CONNECTED_ENTRY)
                    }
                    Err(_) => this.table.update_entry(&pid.0, UNSTABLE_ENTRY),
                    Ok(_) => this.table.update_entry(&pid.0, DISCONNECTED_ENTRY),
                }

                Ok(())
            }
        };

        let result = self
            .connector
            .connect_with_cback(endpoint, &pid, cback)
            .await;

        if let Some(pid) = &pid {
            match result {
                Ok(_) => self.table.update_entry(&pid.0, CONNECTED_ENTRY),
                Err(_) => self.table.update_entry(&pid.0, UNREACHABLE_ENTRY),
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
                let endpoint = Endpoint::Tcp(entry.addr, entry.discovery_port);
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
