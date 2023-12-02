mod connection;
mod endpoint;
mod error;
mod listener;
mod transports;

pub use {
    connection::{dial, Conn, Connection, ToConn},
    endpoint::{Addr, Endpoint, Port},
    listener::{listen, ConnListener, Listener, ToListener},
    transports::{
        tcp::{dial_tcp, listen_tcp, TcpConn},
        tls,
        udp::{dial_udp, listen_udp, UdpConn},
        unix::{dial_unix, listen_unix, UnixConn},
    },
};

use error::{Error, Result};

/// Represents karyon's Net Error
pub use error::Error as NetError;

/// Represents karyon's Net Result
pub use error::Result as NetResult;
