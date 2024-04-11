pub mod codec;
mod connection;
mod endpoint;
mod error;
mod listener;
mod stream;
mod transports;

pub use {
    connection::{Conn, Connection, ToConn},
    endpoint::{Addr, Endpoint, Port, ToEndpoint},
    listener::{ConnListener, Listener, ToListener},
    transports::{tcp, tls, udp, unix, ws},
};

/// Represents karyon's Net Error
pub use error::Error;

/// Represents karyon's Net Result
pub use error::Result;
