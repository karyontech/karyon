use smol::{channel::Sender, lock::Mutex};
use std::{collections::VecDeque, fmt, sync::Arc};

use karyons_core::async_utils::CondVar;

use karyons_net::Conn;

/// Defines the direction of a network connection.
#[derive(Clone, Debug)]
pub enum ConnDirection {
    Inbound,
    Outbound,
}

impl fmt::Display for ConnDirection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ConnDirection::Inbound => write!(f, "Inbound"),
            ConnDirection::Outbound => write!(f, "Outbound"),
        }
    }
}

pub struct NewConn {
    pub direction: ConnDirection,
    pub conn: Conn,
    pub disconnect_signal: Sender<()>,
}

/// Connection queue
pub struct ConnQueue {
    queue: Mutex<VecDeque<NewConn>>,
    conn_available: CondVar,
}

impl ConnQueue {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(VecDeque::new()),
            conn_available: CondVar::new(),
        })
    }

    /// Push a connection into the queue and wait for the disconnect signal
    pub async fn handle(&self, conn: Conn, direction: ConnDirection) {
        let (disconnect_signal, chan) = smol::channel::bounded(1);
        let new_conn = NewConn {
            direction,
            conn,
            disconnect_signal,
        };
        self.queue.lock().await.push_back(new_conn);
        self.conn_available.signal();
        let _ = chan.recv().await;
    }

    /// Receive the next connection in the queue
    pub async fn next(&self) -> NewConn {
        let mut queue = self.queue.lock().await;
        while queue.is_empty() {
            queue = self.conn_available.wait(queue).await;
        }
        queue.pop_front().unwrap()
    }
}
