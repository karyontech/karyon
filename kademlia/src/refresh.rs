use std::{sync::Arc, time::Duration};

#[cfg(target_family = "wasm")]
use log::warn;
use log::{error, trace};

use karyon_core::{
    async_runtime::Executor,
    async_util::{TaskGroup, TaskResult},
};

use karyon_net::Endpoint;

use karyon_p2p::{
    Action, Config, ConnDirection, ConnectorRef, Error, PeerID, Result, Subject, VersionInt,
    monitor::{ConnectionKind, Monitor},
    pick_endpoint,
    util::decode,
};

#[cfg(not(target_family = "wasm"))]
use karyon_p2p::Protocol;

use crate::{
    KademliaConfigRef, SUPPORTED_LOOKUP_PROTOCOLS,
    event::DiscoveryKind,
    lookup::{KadConnRef, send_ping_msg, send_shutdown_msg},
    messages::{KadNetCmd, KadNetMsg, KadNetMsgCodec, PongMsg},
    routing_table::{BucketEntry, Entry, PENDING_ENTRY, RoutingTableRef, UNREACHABLE_ENTRY},
};

// The UDP refresh. No UDP on wasm.
#[cfg(not(target_family = "wasm"))]
mod udp;

/// Maximum failures for an entry before removing it from the routing table.
pub const MAX_FAILURES: u32 = 3;

// Max entries pulled from each bucket per refresh round.
const REFRESH_PER_BUCKET: usize = 8;

pub struct RefreshService {
    /// Routing table
    table: RoutingTableRef,

    /// UDP listen endpoint (None when no udp discovery endpoint is configured).
    listen_endpoint: Option<Endpoint>,

    /// Managing spawned tasks.
    task_group: TaskGroup,

    /// The node's configuration.
    config: Arc<Config>,

    /// The Kademlia configuration.
    kconfig: KademliaConfigRef,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,

    /// The lookup connector, to ping over the lookup protocol when UDP
    /// can not be used.
    connector: ConnectorRef<KadNetMsgCodec>,
}

/// `Arc` reference to a `RefreshService`.
pub(crate) type RefreshServiceRef = Arc<RefreshService>;

impl RefreshService {
    /// Creates a new refresh service
    pub fn new(
        config: Arc<Config>,
        kconfig: KademliaConfigRef,
        table: RoutingTableRef,
        monitor: Arc<Monitor>,
        listen_endpoint: Option<Endpoint>,
        connector: ConnectorRef<KadNetMsgCodec>,
        executor: Executor,
    ) -> Self {
        Self {
            table,
            listen_endpoint,
            task_group: TaskGroup::with_executor(executor.clone()),
            config,
            kconfig,
            monitor,
            connector,
        }
    }

    /// Start the refresh service
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        #[cfg(not(target_family = "wasm"))]
        if let Some(endpoint) = self.listen_endpoint.clone() {
            self.task_group.spawn_then(
                {
                    let this = self.clone();
                    async move { this.listen_loop(endpoint).await }
                },
                |res| async move {
                    if let TaskResult::Completed(Err(err)) = res {
                        error!("Listen loop stopped: {err}");
                    }
                },
            );
        }

        #[cfg(target_family = "wasm")]
        if let Some(endpoint) = &self.listen_endpoint {
            warn!("No UDP on wasm; refresh pings use the lookup protocol, not {endpoint}");
        }

        self.task_group.spawn_then(
            {
                let this = self.clone();
                async move { this.refresh_loop().await }
            },
            |res| async move {
                if let TaskResult::Completed(Err(err)) = res {
                    error!("Refresh loop stopped: {err}");
                }
            },
        );

        Ok(())
    }

    /// Shuts down the refresh service
    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    /// Periodically refresh the routing table by pinging the oldest entries.
    async fn refresh_loop(self: Arc<Self>) -> Result<()> {
        loop {
            self.task_group
                .executor()
                .sleep(Duration::from_secs(self.kconfig.refresh_interval))
                .await;
            trace!("Start refreshing the routing table...");

            self.monitor.notify(DiscoveryKind::RefreshStarted).await;

            let entries = self.table.refresh_candidates(REFRESH_PER_BUCKET);
            let succeeded = self.clone().do_refresh(&entries).await;

            // Empty sweep counts as success with 0 — fail only when at
            // least one entry was tried and all failed.
            if !entries.is_empty() && succeeded == 0 {
                self.monitor.notify(DiscoveryKind::RefreshFailed).await;
            } else {
                self.monitor
                    .notify(DiscoveryKind::RefreshSucceeded(succeeded))
                    .await;
            }
        }
    }

    /// Iterates over the entries and initiates a connection. Returns
    /// the number of entries refreshed successfully.
    async fn do_refresh(self: Arc<Self>, entries: &[BucketEntry]) -> usize {
        use futures_util::stream::{FuturesUnordered, StreamExt};
        let mut succeeded = 0;
        for chunk in entries.chunks(16) {
            let mut tasks = FuturesUnordered::new();
            for bucket_entry in chunk {
                if bucket_entry.failures >= MAX_FAILURES {
                    let pid = bucket_entry.entry.key.into();
                    self.table.remove_entry(&bucket_entry.entry.key);
                    self.monitor.notify(DiscoveryKind::EntryEvicted(pid)).await;
                    continue;
                }
                tasks.push(self.clone().refresh_entry(bucket_entry.clone()))
            }
            while let Some(ok) = tasks.next().await {
                if ok {
                    succeeded += 1;
                }
            }
        }
        succeeded
    }

    /// Refresh a specific entry by pinging it over UDP. Returns true
    /// if the ping succeeded.
    async fn refresh_entry(self: Arc<Self>, bucket_entry: BucketEntry) -> bool {
        let key = &bucket_entry.entry.key;
        match self.connect(&bucket_entry.entry).await {
            Ok(_) => {
                self.table.update_entry(key, PENDING_ENTRY);
                true
            }
            Err(err) => {
                trace!("Failed to refresh entry {key:?}: {err}");
                if bucket_entry.failures >= MAX_FAILURES {
                    let pid = (*key).into();
                    self.table.remove_entry(key);
                    self.monitor.notify(DiscoveryKind::EntryEvicted(pid)).await;
                    return false;
                }
                self.table.update_entry(key, UNREACHABLE_ENTRY);
                false
            }
        }
    }

    /// Pings the entry: over UDP with retries, or over the lookup protocol
    /// when the node has a user transport (no raw UDP besides it), the
    /// entry has no UDP address, or on wasm (no UDP).
    async fn connect(&self, entry: &Entry) -> Result<()> {
        #[cfg(not(target_family = "wasm"))]
        if self.config.transport.is_none()
            && let Some(endpoint) = pick_endpoint(&entry.discovery_addrs, &[Protocol::Udp], None)
        {
            self.allow_probe(&endpoint)?;
            return self.udp_ping(endpoint).await;
        }

        let endpoint = pick_endpoint(
            &entry.discovery_addrs,
            SUPPORTED_LOOKUP_PROTOCOLS,
            self.config.transport.as_ref(),
        )
        .ok_or(Error::Lookup("No discovery address available".into()))?;
        self.allow_probe(&endpoint)?;
        self.stream_ping(endpoint, entry.key.into()).await
    }

    /// Pings the peer at `endpoint` over the lookup protocol.
    async fn stream_ping(&self, endpoint: Endpoint, peer_id: PeerID) -> Result<()> {
        // The slot is released when `_slot` drops at the end.
        let (mut conn, _slot) = self.connector.connect(&endpoint, &Some(peer_id)).await?;
        let t = Duration::from_secs(self.kconfig.lookup_response_timeout);
        let ex = self.task_group.executor();
        let result = ping_conn(&mut conn, &self.config.version.v, t, ex).await;

        self.monitor
            .notify(ConnectionKind::Disconnected(endpoint))
            .await;

        result
    }

    fn allow_probe(&self, endpoint: &Endpoint) -> Result<()> {
        let allowed = self.config.access_control.allow(
            &Subject::Endpoint(endpoint),
            Action::Probe(ConnDirection::Outbound),
        );
        if allowed {
            Ok(())
        } else {
            Err(Error::AccessDenied)
        }
    }
}

/// Pings the peer on a lookup connection, checks its Pong and closes the
/// exchange.
async fn ping_conn(
    conn: &mut KadConnRef,
    version: &VersionInt,
    response_timeout: Duration,
    executor: &Executor,
) -> Result<()> {
    let ping_msg = send_ping_msg(conn, version).await?;
    let msg: KadNetMsg = executor
        .timeout(response_timeout, conn.recv_msg())
        .await??;
    match msg.header.command {
        KadNetCmd::Pong => {
            let (pong_msg, _) = decode::<PongMsg>(&msg.payload)?;
            if ping_msg.nonce != pong_msg.0 {
                return Err(Error::InvalidPongMsg);
            }
        }
        c => return Err(Error::InvalidMsg(format!("Unexpected msg: {c:?}"))),
    }
    send_shutdown_msg(conn).await
}
