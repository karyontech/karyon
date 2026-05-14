use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use log::{error, info, trace};
use rand::{rngs::OsRng, TryRngCore};

use karyon_core::{
    async_runtime::Executor,
    async_util::{sleep, timeout, Backoff, TaskGroup, TaskResult},
};

use karyon_net::{udp, Endpoint};

use crate::{
    discovery::kademlia::{
        messages::RefreshMsg,
        routing_table::{BucketEntry, Entry, RoutingTable, PENDING_ENTRY, UNREACHABLE_ENTRY},
    },
    message::{pick_endpoint, Protocol},
    monitor::{ConnectionKind, DiscoveryKind, Monitor},
    util::{decode, encode},
    Config, Error, Result,
};

/// Maximum failures for an entry before removing it from the routing table.
pub const MAX_FAILURES: u32 = 3;

// Max UDP datagram payload size for refresh messages.
const MAX_UDP_BUF: usize = 1024;

// Max entries pulled from each bucket per refresh round.
const REFRESH_PER_BUCKET: usize = 8;

// Token-bucket parameters for the per-IP rate limit on incoming pings.
// Capacity allows small bursts; refill rate caps sustained traffic.
const RL_CAPACITY: u32 = 5;
const RL_REFILL_PER_SEC: f64 = 0.5;

/// Per-IP token bucket used by `listen_to_ping_msg` to drop floods.
struct RateBucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateBucket {
    fn new() -> Self {
        Self {
            tokens: RL_CAPACITY as f64,
            last_refill: Instant::now(),
        }
    }

    /// Refill and try to take one token. Returns true if allowed.
    fn allow(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * RL_REFILL_PER_SEC).min(RL_CAPACITY as f64);
        self.last_refill = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

pub struct RefreshService {
    /// Routing table
    table: Arc<RoutingTable>,

    /// UDP listen endpoint (None when no udp discovery endpoint is configured).
    listen_endpoint: Option<Endpoint>,

    /// Managing spawned tasks.
    task_group: TaskGroup,

    /// Holds the configuration for the P2P network.
    config: Arc<Config>,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,
}

impl RefreshService {
    /// Creates a new refresh service
    pub fn new(
        config: Arc<Config>,
        table: Arc<RoutingTable>,
        monitor: Arc<Monitor>,
        listen_endpoint: Option<Endpoint>,
        executor: Executor,
    ) -> Self {
        Self {
            table,
            listen_endpoint,
            task_group: TaskGroup::with_executor(executor.clone()),
            config,
            monitor,
        }
    }

    /// Start the refresh service
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        if let Some(endpoint) = self.listen_endpoint.clone() {
            self.task_group.spawn(
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

        self.task_group.spawn(
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
            sleep(Duration::from_secs(self.config.refresh_interval)).await;
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

    /// Send a ping over UDP with retries.
    async fn connect(&self, entry: &Entry) -> Result<()> {
        let mut retry = 0;
        let supported = [Protocol::Udp];
        let endpoint = pick_endpoint(&entry.discovery_addrs, &supported)
            .ok_or(Error::Lookup("No UDP discovery address available".into()))?;
        let conn = udp::dial(&endpoint, Default::default()).await?;
        let peer_addr = SocketAddr::try_from(endpoint.clone())?;
        let backoff = Backoff::new(100, 5000);
        while retry < self.config.refresh_connect_retries {
            match self.send_ping_msg(&conn, peer_addr).await {
                Ok(()) => return Ok(()),
                Err(Error::Timeout) => {
                    retry += 1;
                    backoff.sleep().await;
                }
                Err(err) => {
                    return Err(err);
                }
            }
        }
        Err(Error::Timeout)
    }

    /// Listen on UDP for Ping messages.
    async fn listen_loop(self: Arc<Self>, endpoint: Endpoint) -> Result<()> {
        let conn = match udp::listen(&endpoint, Default::default()).await {
            Ok(c) => {
                self.monitor
                    .notify(ConnectionKind::Listening(endpoint.clone()))
                    .await;
                c
            }
            Err(err) => {
                self.monitor
                    .notify(ConnectionKind::ListenFailed(endpoint.clone()))
                    .await;
                return Err(err.into());
            }
        };
        info!("Start listening on {endpoint}");

        // Per-IP rate limit. Lives on the listen task, no lock needed:
        // listen_to_ping_msg is only ever called from this loop.
        let mut rate_limiter: HashMap<IpAddr, RateBucket> = HashMap::new();

        loop {
            let res = self.listen_to_ping_msg(&conn, &mut rate_limiter).await;
            if let Err(err) = res {
                trace!("Failed to handle ping msg {err}");
                self.monitor.notify(ConnectionKind::AcceptFailed).await;
            }
        }
    }

    /// Listen for a Ping message and respond with a Pong message.
    /// Drops pings from sources not in the routing table or that
    /// exceed the per-IP rate limit.
    async fn listen_to_ping_msg(
        &self,
        conn: &udp::UdpConn,
        rate_limiter: &mut HashMap<IpAddr, RateBucket>,
    ) -> Result<()> {
        let mut buf = vec![0u8; MAX_UDP_BUF];
        let (n, sender) = conn.recv_from(&mut buf).await?;

        let sender_ip = sender.ip();
        if !self.table.has_discovery_ip(&sender_ip) {
            trace!("Drop refresh ping from unknown source {sender_ip}");
            return Ok(());
        }

        let allowed = rate_limiter
            .entry(sender_ip)
            .or_insert_with(RateBucket::new)
            .allow();
        if !allowed {
            trace!("Drop rate-limited refresh ping from {sender_ip}");
            return Ok(());
        }

        let sender_ep = Endpoint::new_udp_addr(sender);
        self.monitor
            .notify(ConnectionKind::Accepted(sender_ep.clone()))
            .await;

        let (msg, _) = decode::<RefreshMsg>(&buf[..n])?;
        match msg {
            RefreshMsg::Ping(m) => {
                let pong_msg = RefreshMsg::Pong(m);
                let encoded = encode(&pong_msg)?;
                conn.send_to(&encoded, sender).await?;
            }
            RefreshMsg::Pong(_) => return Err(Error::InvalidMsg("Unexpected pong msg".into())),
        }

        self.monitor
            .notify(ConnectionKind::Disconnected(sender_ep))
            .await;
        Ok(())
    }

    /// Sends a Ping msg and waits for the Pong response.
    async fn send_ping_msg(&self, conn: &udp::UdpConn, peer_addr: SocketAddr) -> Result<()> {
        let mut nonce: [u8; 32] = [0; 32];
        OsRng.try_fill_bytes(&mut nonce)?;

        let ping = RefreshMsg::Ping(nonce);
        let encoded = encode(&ping)?;
        conn.send_to(&encoded, peer_addr).await?;

        let t = Duration::from_secs(self.config.refresh_response_timeout);
        let mut buf = vec![0u8; MAX_UDP_BUF];
        let (n, _) = timeout(t, conn.recv_from(&mut buf)).await??;
        let (msg, _) = decode::<RefreshMsg>(&buf[..n])?;

        match msg {
            RefreshMsg::Pong(n) => {
                if n != nonce {
                    return Err(Error::InvalidPongMsg);
                }
                Ok(())
            }
            _ => Err(Error::InvalidMsg("Unexpected ping msg".into())),
        }
    }
}
