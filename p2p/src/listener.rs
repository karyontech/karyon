use std::{future::Future, sync::Arc};

use log::{debug, error, info};

use karyon_core::{
    async_runtime::Executor,
    async_util::{TaskGroup, TaskResult},
    crypto::KeyPair,
};

use karyon_net::{tcp, tls, Conn, Endpoint};

use crate::{
    codec::NetMsgCodec,
    message::NetMsg,
    monitor::{ConnEvent, Monitor},
    slots::ConnectionSlots,
    tls_config::tls_server_config,
    Error, Result,
};

/// Responsible for creating inbound connections with other peers.
pub struct Listener {
    /// Identity Key pair
    key_pair: KeyPair,

    /// Managing spawned tasks.
    task_group: TaskGroup,

    /// Manages available inbound slots.
    connection_slots: Arc<ConnectionSlots>,

    /// Enables secure connection.
    enable_tls: bool,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,
}

impl Listener {
    /// Creates a new Listener
    pub fn new(
        key_pair: &KeyPair,
        connection_slots: Arc<ConnectionSlots>,
        enable_tls: bool,
        monitor: Arc<Monitor>,
        ex: Executor,
    ) -> Arc<Self> {
        Arc::new(Self {
            key_pair: key_pair.clone(),
            connection_slots,
            task_group: TaskGroup::with_executor(ex),
            enable_tls,
            monitor,
        })
    }

    /// Starts a listener on the given `endpoint`. For each incoming connection
    /// that is accepted, it invokes the provided `callback`, and pass the
    /// connection to the callback.
    ///
    /// Returns the resloved listening endpoint.
    pub async fn start<Fut>(
        self: &Arc<Self>,
        endpoint: Endpoint,
        // https://github.com/rust-lang/rfcs/pull/2132
        callback: impl FnOnce(Conn<NetMsg>) -> Fut + Clone + Send + 'static,
    ) -> Result<Endpoint>
    where
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let listener = match self.listen(&endpoint).await {
            Ok(listener) => {
                self.monitor
                    .notify(ConnEvent::Listening(endpoint.clone()))
                    .await;
                listener
            }
            Err(err) => {
                error!("Failed to listen on {endpoint}: {err}");
                self.monitor.notify(ConnEvent::ListenFailed(endpoint)).await;
                return Err(err);
            }
        };

        let resolved_endpoint = listener.local_endpoint().map_err(Error::from)?;

        info!("Start listening on {resolved_endpoint}");

        let selfc = self.clone();
        self.task_group
            .spawn(selfc.listen_loop(listener, callback), |_| async {});
        Ok(resolved_endpoint)
    }

    /// Shuts down the listener
    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    async fn listen_loop<Fut>(
        self: Arc<Self>,
        listener: karyon_net::Listener<NetMsg>,
        callback: impl FnOnce(Conn<NetMsg>) -> Fut + Clone + Send + 'static,
    ) where
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        loop {
            // Wait for an available inbound slot.
            self.connection_slots.wait_for_slot().await;
            let result = listener.accept().await;

            let (conn, endpoint) = match result {
                Ok(c) => {
                    let endpoint = match c.peer_endpoint() {
                        Ok(ep) => ep,
                        Err(err) => {
                            self.monitor.notify(ConnEvent::AcceptFailed).await;
                            error!("Failed to accept a new connection: {err}");
                            continue;
                        }
                    };

                    self.monitor
                        .notify(ConnEvent::Accepted(endpoint.clone()))
                        .await;
                    (c, endpoint)
                }
                Err(err) => {
                    error!("Failed to accept a new connection: {err}");
                    self.monitor.notify(ConnEvent::AcceptFailed).await;
                    continue;
                }
            };

            self.connection_slots.add();

            let selfc = self.clone();
            let on_disconnect = |res| async move {
                if let TaskResult::Completed(Err(err)) = res {
                    debug!("Inbound connection dropped: {err}");
                }
                selfc
                    .monitor
                    .notify(ConnEvent::Disconnected(endpoint))
                    .await;
                selfc.connection_slots.remove().await;
            };

            let callback = callback.clone();
            self.task_group.spawn(callback(conn), on_disconnect);
        }
    }

    async fn listen(&self, endpoint: &Endpoint) -> Result<karyon_net::Listener<NetMsg>> {
        if self.enable_tls {
            let tls_config = tls::ServerTlsConfig {
                tcp_config: Default::default(),
                server_config: tls_server_config(&self.key_pair)?,
            };
            tls::listen(endpoint, tls_config, NetMsgCodec::new())
                .await
                .map(|l| Box::new(l) as karyon_net::Listener<NetMsg>)
        } else {
            tcp::listen(endpoint, tcp::TcpConfig::default(), NetMsgCodec::new())
                .await
                .map(|l| Box::new(l) as karyon_net::Listener<NetMsg>)
        }
        .map_err(Error::KaryonNet)
    }
}
