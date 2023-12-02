use std::{future::Future, sync::Arc};

use log::{error, trace, warn};

use karyon_core::{
    async_util::{Backoff, TaskGroup, TaskResult},
    crypto::KeyPair,
    GlobalExecutor,
};
use karyon_net::{dial, tls, Conn, Endpoint, NetError};

use crate::{
    monitor::{ConnEvent, Monitor},
    slots::ConnectionSlots,
    tls_config::tls_client_config,
    Error, PeerID, Result,
};

static DNS_NAME: &str = "karyons.org";

/// Responsible for creating outbound connections with other peers.
pub struct Connector {
    /// Identity Key pair
    key_pair: KeyPair,

    /// Managing spawned tasks.
    task_group: TaskGroup<'static>,

    /// Manages available outbound slots.
    connection_slots: Arc<ConnectionSlots>,

    /// The maximum number of retries allowed before successfully
    /// establishing a connection.
    max_retries: usize,

    /// Enables secure connection.
    enable_tls: bool,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,
}

impl Connector {
    /// Creates a new Connector
    pub fn new(
        key_pair: &KeyPair,
        max_retries: usize,
        connection_slots: Arc<ConnectionSlots>,
        enable_tls: bool,
        monitor: Arc<Monitor>,
        ex: GlobalExecutor,
    ) -> Arc<Self> {
        Arc::new(Self {
            key_pair: key_pair.clone(),
            max_retries,
            task_group: TaskGroup::new(ex),
            monitor,
            connection_slots,
            enable_tls,
        })
    }

    /// Shuts down the connector
    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    /// Establish a connection to the specified `endpoint`. If the connection
    /// attempt fails, it performs a backoff and retries until the maximum allowed
    /// number of retries is exceeded. On a successful connection, it returns a
    /// `Conn` instance.
    ///
    /// This method will block until it finds an available slot.
    pub async fn connect(&self, endpoint: &Endpoint, peer_id: &Option<PeerID>) -> Result<Conn> {
        self.connection_slots.wait_for_slot().await;
        self.connection_slots.add();

        let mut retry = 0;
        let backoff = Backoff::new(500, 2000);
        while retry < self.max_retries {
            match self.dial(endpoint, peer_id).await {
                Ok(conn) => {
                    self.monitor
                        .notify(&ConnEvent::Connected(endpoint.clone()).into())
                        .await;
                    return Ok(conn);
                }
                Err(err) => {
                    error!("Failed to establish a connection to {endpoint}: {err}");
                }
            }

            self.monitor
                .notify(&ConnEvent::ConnectRetried(endpoint.clone()).into())
                .await;

            backoff.sleep().await;

            warn!("try to reconnect {endpoint}");
            retry += 1;
        }

        self.monitor
            .notify(&ConnEvent::ConnectFailed(endpoint.clone()).into())
            .await;

        self.connection_slots.remove().await;
        Err(NetError::Timeout.into())
    }

    /// Establish a connection to the given `endpoint`. For each new connection,
    /// it invokes the provided `callback`, and pass the connection to the callback.
    pub async fn connect_with_cback<Fut>(
        self: &Arc<Self>,
        endpoint: &Endpoint,
        peer_id: &Option<PeerID>,
        callback: impl FnOnce(Conn) -> Fut + Send + 'static,
    ) -> Result<()>
    where
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let conn = self.connect(endpoint, peer_id).await?;

        let selfc = self.clone();
        let endpoint = endpoint.clone();
        let on_disconnect = |res| async move {
            if let TaskResult::Completed(Err(err)) = res {
                trace!("Outbound connection dropped: {err}");
            }
            selfc
                .monitor
                .notify(&ConnEvent::Disconnected(endpoint.clone()).into())
                .await;
            selfc.connection_slots.remove().await;
        };

        self.task_group.spawn(callback(conn), on_disconnect);

        Ok(())
    }

    async fn dial(&self, endpoint: &Endpoint, peer_id: &Option<PeerID>) -> Result<Conn> {
        if self.enable_tls {
            let tls_config = tls_client_config(&self.key_pair, peer_id.clone())?;
            tls::dial(endpoint, tls_config, DNS_NAME).await
        } else {
            dial(endpoint).await
        }
        .map_err(Error::KaryonNet)
    }
}
