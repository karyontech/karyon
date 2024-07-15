use std::{sync::Arc, time::Duration};

use bincode::{Decode, Encode};
use log::{error, info, trace};
use parking_lot::RwLock;
use rand::{rngs::OsRng, RngCore};

use karyon_core::{
    async_runtime::Executor,
    async_util::{sleep, timeout, Backoff, TaskGroup, TaskResult},
};

use karyon_net::{udp, Connection, Endpoint, Error as NetError};

use crate::{
    codec::RefreshMsgCodec,
    message::RefreshMsg,
    monitor::{ConnEvent, DiscvEvent, Monitor},
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
    table: Arc<RoutingTable>,

    /// Resolved listen endpoint
    listen_endpoint: RwLock<Option<Endpoint>>,

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
        executor: Executor,
    ) -> Self {
        Self {
            table,
            listen_endpoint: RwLock::new(None),
            task_group: TaskGroup::with_executor(executor.clone()),
            config,
            monitor,
        }
    }

    /// Start the refresh service
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        if let Some(endpoint) = self.listen_endpoint.read().as_ref() {
            let endpoint = endpoint.clone();
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

    /// Set the resolved listen endpoint.
    pub fn set_listen_endpoint(&self, resolved_endpoint: &Endpoint) -> Result<()> {
        let resolved_endpoint = Endpoint::Udp(
            resolved_endpoint.addr()?.clone(),
            self.config.discovery_port,
        );
        *self.listen_endpoint.write() = Some(resolved_endpoint);
        Ok(())
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

            self.monitor.notify(DiscvEvent::RefreshStarted).await;

            let mut entries: Vec<BucketEntry> = vec![];
            for bucket in self.table.buckets() {
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

    /// Iterates over the entries and initiates a connection.
    async fn do_refresh(self: Arc<Self>, entries: &[BucketEntry]) {
        use futures_util::stream::{FuturesUnordered, StreamExt};
        // Enforce a maximum of 16 connections.
        for chunk in entries.chunks(16) {
            let mut tasks = FuturesUnordered::new();
            for bucket_entry in chunk {
                if bucket_entry.failures >= MAX_FAILURES {
                    self.table.remove_entry(&bucket_entry.entry.key);
                    continue;
                }

                tasks.push(self.clone().refresh_entry(bucket_entry.clone()))
            }

            while tasks.next().await.is_some() {}
        }
    }

    /// Initiates refresh for a specific entry within the routing table. It
    /// updates the routing table according to the result.
    async fn refresh_entry(self: Arc<Self>, bucket_entry: BucketEntry) {
        let key = &bucket_entry.entry.key;
        match self.connect(&bucket_entry.entry).await {
            Ok(_) => {
                self.table.update_entry(key, PENDING_ENTRY);
            }
            Err(err) => {
                trace!("Failed to refresh entry {:?}: {err}", key);
                if bucket_entry.failures >= MAX_FAILURES {
                    self.table.remove_entry(key);
                    return;
                }
                self.table.update_entry(key, UNREACHABLE_ENTRY);
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
                    .notify(ConnEvent::Listening(endpoint.clone()))
                    .await;
                c
            }
            Err(err) => {
                self.monitor
                    .notify(ConnEvent::ListenFailed(endpoint.clone()))
                    .await;
                return Err(err.into());
            }
        };
        info!("Start listening on {endpoint}");

        loop {
            let res = self.listen_to_ping_msg(&conn).await;
            if let Err(err) = res {
                trace!("Failed to handle ping msg {err}");
                self.monitor.notify(ConnEvent::AcceptFailed).await;
            }
        }
    }

    /// Listen to receive a Ping message and respond with a Pong message.
    async fn listen_to_ping_msg(&self, conn: &udp::UdpConn<RefreshMsgCodec>) -> Result<()> {
        let (msg, endpoint) = conn.recv().await?;
        self.monitor
            .notify(ConnEvent::Accepted(endpoint.clone()))
            .await;

        match msg {
            RefreshMsg::Ping(m) => {
                let pong_msg = RefreshMsg::Pong(m);
                conn.send((pong_msg, endpoint.clone())).await?;
            }
            RefreshMsg::Pong(_) => return Err(Error::InvalidMsg("Unexpected pong msg".into())),
        }

        self.monitor.notify(ConnEvent::Disconnected(endpoint)).await;
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
