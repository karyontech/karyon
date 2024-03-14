mod connection;
mod endpoint;
mod error;
mod listener;
mod transports;

pub use {
    connection::{dial, Conn, Connection, ToConn},
    endpoint::{Addr, Endpoint, Port},
    listener::{listen, ConnListener, Listener, ToListener},
    transports::{tcp, tls, udp, unix, ws},
};

use error::{Error, Result};

/// Represents karyon's Net Error
pub use error::Error as NetError;

/// Represents karyon's Net Result
pub use error::Result as NetResult;
