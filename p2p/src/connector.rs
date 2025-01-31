use std::{future::Future, sync::Arc};

use log::{error, trace, warn};

use karyon_core::{
    async_runtime::Executor,
    async_util::{Backoff, TaskGroup, TaskResult},
    crypto::KeyPair,
};
use karyon_net::{tcp, tls, Endpoint};

use crate::{
    codec::NetMsgCodec,
    monitor::{ConnEvent, Monitor},
    slots::ConnectionSlots,
    tls_config::tls_client_config,
    ConnRef, Error, PeerID, Result,
};

static DNS_NAME: &str = "karyontech.net";

/// Responsible for creating outbound connections with other peers.
pub struct Connector {
    /// Identity Key pair
    key_pair: KeyPair,

    /// Managing spawned tasks.
    task_group: TaskGroup,

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
        ex: Executor,
    ) -> Arc<Self> {
        Arc::new(Self {
            key_pair: key_pair.clone(),
            max_retries,
            task_group: TaskGroup::with_executor(ex),
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
    pub async fn connect(&self, endpoint: &Endpoint, peer_id: &Option<PeerID>) -> Result<ConnRef> {
        self.connection_slots.wait_for_slot().await;
        self.connection_slots.add();

        let mut retry = 0;
        let backoff = Backoff::new(500, 2000);
        while retry < self.max_retries {
            match self.dial(endpoint, peer_id).await {
                Ok(conn) => {
                    self.monitor
                        .notify(ConnEvent::Connected(endpoint.clone()))
                        .await;
                    return Ok(conn);
                }
                Err(err) => {
                    error!("Failed to establish a connection to {endpoint}: {err}");
                }
            }

            self.monitor
                .notify(ConnEvent::ConnectRetried(endpoint.clone()))
                .await;

            backoff.sleep().await;

            warn!("try to reconnect {endpoint}");
            retry += 1;
        }

        self.monitor
            .notify(ConnEvent::ConnectFailed(endpoint.clone()))
            .await;

        self.connection_slots.remove().await;
        Err(Error::Timeout)
    }

    /// Establish a connection to the given `endpoint`. For each new connection,
    /// it invokes the provided `callback`, and pass the connection to the callback.
    pub async fn connect_with_cback<Fut>(
        self: &Arc<Self>,
        endpoint: &Endpoint,
        peer_id: &Option<PeerID>,
        callback: impl FnOnce(ConnRef) -> Fut + Send + 'static,
    ) -> Result<()>
    where
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let conn = self.connect(endpoint, peer_id).await?;

        let endpoint = endpoint.clone();
        let on_disconnect = {
            let this = self.clone();
            |res| async move {
                if let TaskResult::Completed(Err(err)) = res {
                    trace!("Outbound connection dropped: {err}");
                }
                this.monitor
                    .notify(ConnEvent::Disconnected(endpoint.clone()))
                    .await;
                this.connection_slots.remove().await;
            }
        };

        self.task_group.spawn(callback(conn), on_disconnect);

        Ok(())
    }

    async fn dial(&self, endpoint: &Endpoint, peer_id: &Option<PeerID>) -> Result<ConnRef> {
        if self.enable_tls {
            if !endpoint.is_tcp() && !endpoint.is_tls() {
                return Err(Error::UnsupportedEndpoint(endpoint.to_string()));
            }

            let tls_config = tls::ClientTlsConfig {
                tcp_config: Default::default(),
                client_config: tls_client_config(&self.key_pair, peer_id.clone())?,
                dns_name: DNS_NAME.to_string(),
            };
            let c = tls::dial(endpoint, tls_config, NetMsgCodec::new()).await?;
            Ok(Box::new(c))
        } else {
            if !endpoint.is_tcp() {
                return Err(Error::UnsupportedEndpoint(endpoint.to_string()));
            }

            let c = tcp::dial(endpoint, tcp::TcpConfig::default(), NetMsgCodec::new()).await?;
            Ok(Box::new(c))
        }
    }
}
