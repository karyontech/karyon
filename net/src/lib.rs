pub mod codec;
mod connection;
mod endpoint;
mod error;
mod listener;
#[cfg(feature = "stream")]
mod stream;
mod transports;

pub use {
    connection::{Conn, Connection, ToConn},
    endpoint::{Addr, Endpoint, Port, ToEndpoint},
    listener::{ConnListener, Listener, ToListener},
};

#[cfg(feature = "tcp")]
pub use transports::tcp;

#[cfg(feature = "tls")]
pub use transports::tls;

#[cfg(feature = "ws")]
pub use transports::ws;

#[cfg(feature = "udp")]
pub use transports::udp;

#[cfg(all(feature = "unix", target_family = "unix"))]
pub use transports::unix;

#[cfg(feature = "tls")]
pub use karyon_async_rustls as async_rustls;

/// Represents karyon's Net Error
pub use error::Error;

/// Represents karyon's Net Result
pub use error::Result;
