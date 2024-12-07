#[cfg(target_family = "unix")]
pub use std::os::unix::net::SocketAddr;

#[cfg(all(feature = "smol", target_family = "unix"))]
pub use smol::net::unix::{SocketAddr as UnixSocketAddr, UnixListener, UnixStream};
#[cfg(feature = "smol")]
pub use smol::net::{TcpListener, TcpStream, UdpSocket};

#[cfg(all(feature = "tokio", target_family = "unix"))]
pub use tokio::net::{unix::SocketAddr as UnixSocketAddr, UnixListener, UnixStream};
#[cfg(feature = "tokio")]
pub use tokio::net::{TcpListener, TcpStream, UdpSocket};
