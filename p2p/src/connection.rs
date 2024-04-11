use std::{collections::VecDeque, fmt, sync::Arc};

use async_channel::Sender;

use karyon_core::{async_runtime::lock::Mutex, async_util::CondVar};
use karyon_net::Conn;

use crate::{message::NetMsg, Result};

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
    pub conn: Conn<NetMsg>,
    pub disconnect_signal: Sender<Result<()>>,
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
    pub async fn handle(&self, conn: Conn<NetMsg>, direction: ConnDirection) -> Result<()> {
        let (disconnect_signal, chan) = async_channel::bounded(1);
        let new_conn = NewConn {
            direction,
            conn,
            disconnect_signal,
        };
        self.queue.lock().await.push_back(new_conn);
        self.conn_available.signal();
        if let Ok(result) = chan.recv().await {
            return result;
        }
        Ok(())
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
