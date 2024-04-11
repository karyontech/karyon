pub use std::os::unix::net::SocketAddr;

#[cfg(feature = "smol")]
pub use smol::net::{
    unix::{SocketAddr as UnixSocketAddr, UnixListener, UnixStream},
    TcpListener, TcpStream, UdpSocket,
};

#[cfg(feature = "tokio")]
pub use tokio::net::{
    unix::SocketAddr as UnixSocketAddr, TcpListener, TcpStream, UdpSocket, UnixListener, UnixStream,
};
