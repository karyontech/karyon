use std::{collections::VecDeque, sync::Arc};

use karyon_core::{async_runtime::lock::Mutex, async_util::CondVar};
use karyon_net::Conn;

use crate::{connection::ConnDirection, connection::Connection, message::NetMsg, Result};

/// Connection queue
pub struct ConnQueue {
    queue: Mutex<VecDeque<Connection>>,
    conn_available: CondVar,
}

impl ConnQueue {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(VecDeque::new()),
            conn_available: CondVar::new(),
        })
    }

    /// Handle a connection by pushing it into the queue and wait for the disconnect signal
    pub async fn handle(&self, conn: Conn<NetMsg>, direction: ConnDirection) -> Result<()> {
        let endpoint = conn.peer_endpoint()?;

        let (disconnect_tx, disconnect_rx) = async_channel::bounded(1);
        let new_conn = Connection::new(conn, disconnect_tx, direction, endpoint);

        // Push a new conn to the queue
        self.queue.lock().await.push_back(new_conn);
        self.conn_available.signal();

        // Wait for the disconnect signal from the connection handler
        if let Ok(result) = disconnect_rx.recv().await {
            return result;
        }

        Ok(())
    }

    /// Waits for the next connection in the queue
    pub async fn next(&self) -> Connection {
        let mut queue = self.queue.lock().await;
        while queue.is_empty() {
            queue = self.conn_available.wait(queue).await;
        }
        queue.pop_front().unwrap()
    }
}
