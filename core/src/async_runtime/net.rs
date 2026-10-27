use std::{io, net::SocketAddr as InetSocketAddr};

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

/// Resolves a host name on a blocking thread pool, so a slow DNS server
/// does not block the executor.
pub async fn lookup_host(host: &str, port: u16) -> io::Result<Vec<InetSocketAddr>> {
    #[cfg(feature = "smol")]
    let result = smol::net::resolve((host, port)).await;
    #[cfg(feature = "tokio")]
    let result = tokio::net::lookup_host((host, port))
        .await
        .map(|addrs| addrs.collect());

    result
}
