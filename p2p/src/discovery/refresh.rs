use std::{sync::Arc, time::Duration};

use bincode::{Decode, Encode};
use log::{error, info, trace};
use rand::{rngs::OsRng, RngCore};

use karyon_core::{
    async_runtime::{
        lock::{Mutex, RwLock},
        Executor,
    },
    async_util::{sleep, timeout, Backoff, TaskGroup, TaskResult},
};

use karyon_net::{udp, Connection, Endpoint, Error as NetError};

use crate::{
    codec::RefreshMsgCodec,
    message::RefreshMsg,
    monitor::{ConnEvent, DiscoveryEvent, Monitor},
    routing_table::{BucketEntry, Entry, RoutingTable, PENDING_ENTRY, UNREACHABLE_ENTRY},
    Config, Error, Result,
};

/// Maximum failures for an entry before removing it from the routing table.
pub const MAX_FAILURES: u32 = 3;

#[derive(Decode, Encode, Debug, Clone)]
pub struct PingMsg(pub [u8; 32]);

#[derive(Decode, Encode, Debug)]
pub struct PongMsg(pub [u8; 32]);

pub struct RefreshService {
    /// Routing table
    table: Arc<Mutex<RoutingTable>>,

    /// Resolved listen endpoint
    listen_endpoint: Option<RwLock<Endpoint>>,

    /// Managing spawned tasks.
    task_group: TaskGroup,

    /// A global executor
    executor: Executor,

    /// Holds the configuration for the P2P network.
    config: Arc<Config>,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,
}

impl RefreshService {
    /// Creates a new refresh service
    pub fn new(
        config: Arc<Config>,
        table: Arc<Mutex<RoutingTable>>,
        monitor: Arc<Monitor>,
        executor: Executor,
    ) -> Self {
        let listen_endpoint = config
            .listen_endpoint
            .as_ref()
            .map(|endpoint| RwLock::new(endpoint.clone()));

        Self {
            table,
            listen_endpoint,
            task_group: TaskGroup::with_executor(executor.clone()),
            executor,
            config,
            monitor,
        }
    }

    /// Start the refresh service
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        if let Some(endpoint) = &self.listen_endpoint {
            let endpoint = endpoint.read().await;

            let selfc = self.clone();
            self.task_group
                .spawn(selfc.listen_loop(endpoint.clone()), |res| async move {
                    if let TaskResult::Completed(Err(err)) = res {
                        error!("Listen loop stopped: {err}");
                    }
                });
        }

        let selfc = self.clone();
        self.task_group
            .spawn(selfc.refresh_loop(), |res| async move {
                if let TaskResult::Completed(Err(err)) = res {
                    error!("Refresh loop stopped: {err}");
                }
            });

        Ok(())
    }

    /// Set the resolved listen endpoint.
    pub async fn set_listen_endpoint(&self, resolved_endpoint: &Endpoint) {
        if let Some(endpoint) = &self.listen_endpoint {
            *endpoint.write().await = resolved_endpoint.clone();
        }
    }

    /// Shuts down the refresh service
    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    /// Initiates periodic refreshing of the routing table. This function will
    /// selects the first 8 entries (oldest entries) from each bucket in the
    /// routing table and starts sending Ping messages to the collected entries.
    async fn refresh_loop(self: Arc<Self>) -> Result<()> {
        loop {
            sleep(Duration::from_secs(self.config.refresh_interval)).await;
            trace!("Start refreshing the routing table...");

            self.monitor
                .notify(&DiscoveryEvent::RefreshStarted.into())
                .await;

            let mut entries: Vec<BucketEntry> = vec![];
            for bucket in self.table.lock().await.iter() {
                for entry in bucket
                    .iter()
                    .filter(|e| !e.is_connected() && !e.is_incompatible())
                    .take(8)
                {
                    entries.push(entry.clone())
                }
            }

            self.clone().do_refresh(&entries).await;
        }
    }

    /// Iterates over the entries and spawns a new task for each entry to
    /// initiate a connection attempt.
    async fn do_refresh(self: Arc<Self>, entries: &[BucketEntry]) {
        let ex = &self.executor;
        // Enforce a maximum of 16 concurrent connections.
        for chunk in entries.chunks(16) {
            let mut tasks = Vec::new();
            for bucket_entry in chunk {
                if bucket_entry.failures >= MAX_FAILURES {
                    self.table
                        .lock()
                        .await
                        .remove_entry(&bucket_entry.entry.key);
                    continue;
                }

                tasks.push(ex.spawn(self.clone().refresh_entry(bucket_entry.clone())))
            }

            for task in tasks {
                let _ = task.await;
            }
        }
    }

    /// Initiates refresh for a specific entry within the routing table. It
    /// updates the routing table according to the result.
    async fn refresh_entry(self: Arc<Self>, bucket_entry: BucketEntry) {
        let key = &bucket_entry.entry.key;
        match self.connect(&bucket_entry.entry).await {
            Ok(_) => {
                self.table.lock().await.update_entry(key, PENDING_ENTRY);
            }
            Err(err) => {
                trace!("Failed to refresh entry {:?}: {err}", key);
                let table = &mut self.table.lock().await;
                if bucket_entry.failures >= MAX_FAILURES {
                    table.remove_entry(key);
                    return;
                }
                table.update_entry(key, UNREACHABLE_ENTRY);
            }
        }
    }

    /// Initiates a UDP connection with the entry and attempts to send a Ping
    /// message. If it fails, it retries according to the allowed retries
    /// specified in the Config, with backoff between each retry.
    async fn connect(&self, entry: &Entry) -> Result<()> {
        let mut retry = 0;
        let endpoint = Endpoint::Udp(entry.addr.clone(), entry.discovery_port);
        let conn = udp::dial(&endpoint, Default::default(), RefreshMsgCodec {}).await?;
        let backoff = Backoff::new(100, 5000);
        while retry < self.config.refresh_connect_retries {
            match self.send_ping_msg(&conn, &endpoint).await {
                Ok(()) => return Ok(()),
                Err(Error::KaryonNet(NetError::Timeout)) => {
                    retry += 1;
                    backoff.sleep().await;
                }
                Err(err) => {
                    return Err(err);
                }
            }
        }

        Err(NetError::Timeout.into())
    }

    /// Set up a UDP listener and start listening for Ping messages from other
    /// peers.
    async fn listen_loop(self: Arc<Self>, endpoint: Endpoint) -> Result<()> {
        let conn = match udp::listen(&endpoint, Default::default(), RefreshMsgCodec {}).await {
            Ok(c) => {
                self.monitor
                    .notify(&ConnEvent::Listening(endpoint.clone()).into())
                    .await;
                c
            }
            Err(err) => {
                self.monitor
                    .notify(&ConnEvent::ListenFailed(endpoint.clone()).into())
                    .await;
                return Err(err.into());
            }
        };
        info!("Start listening on {endpoint}");

        loop {
            let res = self.listen_to_ping_msg(&conn).await;
            if let Err(err) = res {
                trace!("Failed to handle ping msg {err}");
                self.monitor.notify(&ConnEvent::AcceptFailed.into()).await;
            }
        }
    }

    /// Listen to receive a Ping message and respond with a Pong message.
    async fn listen_to_ping_msg(&self, conn: &udp::UdpConn<RefreshMsgCodec>) -> Result<()> {
        let (msg, endpoint) = conn.recv().await?;
        self.monitor
            .notify(&ConnEvent::Accepted(endpoint.clone()).into())
            .await;

        match msg {
            RefreshMsg::Ping(m) => {
                let pong_msg = RefreshMsg::Pong(m);
                conn.send((pong_msg, endpoint.clone())).await?;
            }
            RefreshMsg::Pong(_) => return Err(Error::InvalidMsg("Unexpected pong msg".into())),
        }

        self.monitor
            .notify(&ConnEvent::Disconnected(endpoint).into())
            .await;
        Ok(())
    }

    /// Sends a Ping msg and wait to receive the Pong message.
    async fn send_ping_msg(
        &self,
        conn: &udp::UdpConn<RefreshMsgCodec>,
        endpoint: &Endpoint,
    ) -> Result<()> {
        let mut nonce: [u8; 32] = [0; 32];
        RngCore::fill_bytes(&mut OsRng, &mut nonce);
        conn.send((RefreshMsg::Ping(nonce), endpoint.clone()))
            .await?;

        let t = Duration::from_secs(self.config.refresh_response_timeout);
        let (msg, _) = timeout(t, conn.recv()).await??;

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
