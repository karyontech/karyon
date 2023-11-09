use std::{future::Future, sync::Arc};

use log::{error, info, trace};

use karyons_core::{
    async_utils::{TaskGroup, TaskResult},
    Executor,
};

use karyons_net::{listen, Conn, Endpoint, Listener as NetListener};

use crate::{
    monitor::{ConnEvent, Monitor},
    Result,
};

use super::slots::ConnectionSlots;

/// Responsible for creating inbound connections with other peers.
pub struct Listener {
    /// Managing spawned tasks.
    task_group: TaskGroup,

    /// Manages available inbound slots.
    connection_slots: Arc<ConnectionSlots>,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,
}

impl Listener {
    /// Creates a new Listener
    pub fn new(connection_slots: Arc<ConnectionSlots>, monitor: Arc<Monitor>) -> Arc<Self> {
        Arc::new(Self {
            connection_slots,
            task_group: TaskGroup::new(),
            monitor,
        })
    }

    /// Starts a listener on the given `endpoint`. For each incoming connection
    /// that is accepted, it invokes the provided `callback`, and pass the
    /// connection to the callback.
    ///
    /// Returns the resloved listening endpoint.
    pub async fn start<'a, Fut>(
        self: &Arc<Self>,
        ex: Executor<'a>,
        endpoint: Endpoint,
        // https://github.com/rust-lang/rfcs/pull/2132
        callback: impl FnOnce(Conn) -> Fut + Clone + Send + 'a,
    ) -> Result<Endpoint>
    where
        Fut: Future<Output = Result<()>> + Send + 'a,
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
        self.task_group.spawn(
            ex.clone(),
            selfc.listen_loop(ex.clone(), listener, callback),
            |res| async move {
                if let TaskResult::Completed(Err(err)) = res {
                    error!("Listen loop stopped: {endpoint} {err}");
                }
            },
        );
        Ok(resolved_endpoint)
    }

    /// Shuts down the listener
    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    async fn listen_loop<'a, Fut>(
        self: Arc<Self>,
        ex: Executor<'a>,
        listener: Box<dyn NetListener>,
        callback: impl FnOnce(Conn) -> Fut + Clone + Send + 'a,
    ) -> Result<()>
    where
        Fut: Future<Output = Result<()>> + Send + 'a,
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
            self.task_group
                .spawn(ex.clone(), callback(conn), on_disconnect);
        }
    }
}
