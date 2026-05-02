//! Read/write traits over message connections.
//!
//! Both `FramedReader`/`FramedWriter` and `WsReader`/`WsWriter`
//! implement these, so generic code (e.g. JSON-RPC client/server
//! reader and writer tasks) can drive either transport.

use std::future::Future;

use crate::{Endpoint, Result};

/// Read half of a message connection.
pub trait MessageRx: Send + Sync {
    type Message: Send + Sync;

    /// Receive one complete message.
    fn recv_msg(&mut self) -> impl Future<Output = Result<Self::Message>> + Send;

    /// Remote peer address.
    fn peer_endpoint(&self) -> Option<Endpoint>;
}

/// Write half of a message connection.
pub trait MessageTx: Send + Sync {
    type Message: Send + Sync;

    /// Send one complete message.
    fn send_msg(&mut self, msg: Self::Message) -> impl Future<Output = Result<()>> + Send;

    /// Remote peer address.
    fn peer_endpoint(&self) -> Option<Endpoint>;
}
