use std::{future::Future, sync::Arc};

use log::{error, info, trace};

use karyons_core::{
    async_utils::{TaskGroup, TaskResult},
    GlobalExecutor,
};

use karyons_net::{listen, Conn, Endpoint, Listener as NetListener};

use crate::{
    monitor::{ConnEvent, Monitor},
    slots::ConnectionSlots,
    Result,
};

/// Responsible for creating inbound connections with other peers.
pub struct Listener {
    /// Managing spawned tasks.
    task_group: TaskGroup<'static>,

    /// Manages available inbound slots.
    connection_slots: Arc<ConnectionSlots>,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,
}

impl Listener {
    /// Creates a new Listener
    pub fn new(
        connection_slots: Arc<ConnectionSlots>,
        monitor: Arc<Monitor>,
        ex: GlobalExecutor,
    ) -> Arc<Self> {
        Arc::new(Self {
            connection_slots,
            task_group: TaskGroup::new(ex),
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
        callback: impl FnOnce(Conn) -> Fut + Clone + Send + 'static,
    ) -> Result<Endpoint>
    where
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let listener = match listen(&endpoint).await {
            Ok(listener) => {
                self.monitor
                    .notify(&ConnEvent::Listening(endpoint.clone()).into())
                    .await;
                listener
            }
            Err(err) => {
                error!("Failed to listen on {endpoint}: {err}");
                self.monitor
                    .notify(&ConnEvent::ListenFailed(endpoint).into())
                    .await;
                return Err(err.into());
            }
        };

        let resolved_endpoint = listener.local_endpoint()?;

        info!("Start listening on {endpoint}");

        let selfc = self.clone();
        self.task_group
            .spawn(selfc.listen_loop(listener, callback), |res| async move {
                if let TaskResult::Completed(Err(err)) = res {
                    error!("Listen loop stopped: {endpoint} {err}");
                }
            });
        Ok(resolved_endpoint)
    }

    /// Shuts down the listener
    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    async fn listen_loop<Fut>(
        self: Arc<Self>,
        listener: Box<dyn NetListener>,
        callback: impl FnOnce(Conn) -> Fut + Clone + Send + 'static,
    ) -> Result<()>
    where
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        loop {
            // Wait for an available inbound slot.
            self.connection_slots.wait_for_slot().await;
            let result = listener.accept().await;

            let conn = match result {
                Ok(c) => {
                    self.monitor
                        .notify(&ConnEvent::Accepted(c.peer_endpoint()?).into())
                        .await;
                    c
                }
                Err(err) => {
                    error!("Failed to accept a new connection: {err}");
                    self.monitor.notify(&ConnEvent::AcceptFailed.into()).await;
                    return Err(err.into());
                }
            };

            self.connection_slots.add();

            let selfc = self.clone();
            let endpoint = conn.peer_endpoint()?;
            let on_disconnect = |res| async move {
                if let TaskResult::Completed(Err(err)) = res {
                    trace!("Inbound connection dropped: {err}");
                }
                selfc
                    .monitor
                    .notify(&ConnEvent::Disconnected(endpoint).into())
                    .await;
                selfc.connection_slots.remove().await;
            };

            let callback = callback.clone();
            self.task_group.spawn(callback(conn), on_disconnect);
        }
    }
}
