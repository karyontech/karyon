use std::{future::Future, sync::Arc};

use log::{trace, warn};

use karyons_core::{
    async_utils::{Backoff, TaskGroup, TaskResult},
    Executor,
};
use karyons_net::{dial, Conn, Endpoint, NetError};

use crate::{
    monitor::{ConnEvent, Monitor},
    slots::ConnectionSlots,
    Result,
};

/// Responsible for creating outbound connections with other peers.
pub struct Connector {
    /// Managing spawned tasks.
    task_group: TaskGroup,

    /// Manages available outbound slots.
    connection_slots: Arc<ConnectionSlots>,

    /// The maximum number of retries allowed before successfully
    /// establishing a connection.
    max_retries: usize,

    /// Responsible for network and system monitoring.
    monitor: Arc<Monitor>,
}

impl Connector {
    /// Creates a new Connector
    pub fn new(
        max_retries: usize,
        connection_slots: Arc<ConnectionSlots>,
        monitor: Arc<Monitor>,
    ) -> Arc<Self> {
        Arc::new(Self {
            task_group: TaskGroup::new(),
            monitor,
            connection_slots,
            max_retries,
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
    pub async fn connect(&self, endpoint: &Endpoint) -> Result<Conn> {
        self.connection_slots.wait_for_slot().await;
        self.connection_slots.add();

        let mut retry = 0;
        let backoff = Backoff::new(500, 2000);
        while retry < self.max_retries {
            let conn_result = dial(endpoint).await;

            if let Ok(conn) = conn_result {
                self.monitor
                    .notify(&ConnEvent::Connected(endpoint.clone()).into())
                    .await;
                return Ok(conn);
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
    pub async fn connect_with_cback<'a, Fut>(
        self: &Arc<Self>,
        ex: Executor<'a>,
        endpoint: &Endpoint,
        callback: impl FnOnce(Conn) -> Fut + Send + 'a,
    ) -> Result<()>
    where
        Fut: Future<Output = Result<()>> + Send + 'a,
    {
        let conn = self.connect(endpoint).await?;

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

        self.task_group
            .spawn(ex.clone(), callback(conn), on_disconnect);

        Ok(())
    }
}
