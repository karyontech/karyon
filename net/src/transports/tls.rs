use std::sync::Arc;

use async_rustls::{rustls, TlsAcceptor, TlsConnector, TlsStream};
use async_trait::async_trait;
use smol::{
    io::{split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    lock::Mutex,
    net::{TcpListener, TcpStream},
};

use crate::{
    connection::Connection,
    endpoint::{Addr, Endpoint, Port},
    listener::ConnListener,
    Error, Result,
};

/// TLS network connection implementation of the [`Connection`] trait.
pub struct TlsConn {
    inner: TcpStream,
    read: Mutex<ReadHalf<TlsStream<TcpStream>>>,
    write: Mutex<WriteHalf<TlsStream<TcpStream>>>,
}

impl TlsConn {
    /// Creates a new TlsConn
    pub fn new(sock: TcpStream, conn: TlsStream<TcpStream>) -> Self {
        let (read, write) = split(conn);
        Self {
            inner: sock,
            read: Mutex::new(read),
            write: Mutex::new(write),
        }
    }
}

#[async_trait]
impl Connection for TlsConn {
    fn peer_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_tls_addr(&self.inner.peer_addr()?))
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_tls_addr(&self.inner.local_addr()?))
    }

    async fn read(&self, buf: &mut [u8]) -> Result<usize> {
        self.read.lock().await.read(buf).await.map_err(Error::from)
    }

    async fn write(&self, buf: &[u8]) -> Result<usize> {
        self.write
            .lock()
            .await
            .write(buf)
            .await
            .map_err(Error::from)
    }
}

/// Connects to the given TLS address and port.
pub async fn dial_tls(
    addr: &Addr,
    port: &Port,
    config: rustls::ClientConfig,
    dns_name: &str,
) -> Result<TlsConn> {
    let address = format!("{}:{}", addr, port);

    let connector = TlsConnector::from(Arc::new(config));

    let sock = TcpStream::connect(&address).await?;
    sock.set_nodelay(true)?;

    let altname = rustls::ServerName::try_from(dns_name)?;
    let conn = connector.connect(altname, sock.clone()).await?;
    Ok(TlsConn::new(sock, TlsStream::Client(conn)))
}

/// Connects to the given TLS endpoint, returns `Conn` ([`Connection`]).
pub async fn dial(
    endpoint: &Endpoint,
    config: rustls::ClientConfig,
    dns_name: &str,
) -> Result<Box<dyn Connection>> {
    match endpoint {
        Endpoint::Tcp(..) | Endpoint::Tls(..) => {}
        _ => return Err(Error::InvalidEndpoint(endpoint.to_string())),
    }

    dial_tls(endpoint.addr()?, endpoint.port()?, config, dns_name)
        .await
        .map(|c| Box::new(c) as Box<dyn Connection>)
}
/// Tls network listener implementation of the `Listener` [`ConnListener`] trait.
pub struct TlsListener {
    acceptor: TlsAcceptor,
    listener: TcpListener,
}

#[async_trait]
impl ConnListener for TlsListener {
    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_tls_addr(&self.listener.local_addr()?))
    }

    async fn accept(&self) -> Result<Box<dyn Connection>> {
        let (sock, _) = self.listener.accept().await?;
        sock.set_nodelay(true)?;
        let conn = self.acceptor.accept(sock.clone()).await?;
        Ok(Box::new(TlsConn::new(sock, TlsStream::Server(conn))))
    }
}

/// Listens on the given TLS address and port.
pub async fn listen_tls(
    addr: &Addr,
    port: &Port,
    config: rustls::ServerConfig,
) -> Result<TlsListener> {
    let address = format!("{}:{}", addr, port);
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind(&address).await?;
    Ok(TlsListener { acceptor, listener })
}

/// Listens on the given TLS endpoint, returns `Listener` [`ConnListener`].
pub async fn listen(
    endpoint: &Endpoint,
    config: rustls::ServerConfig,
) -> Result<Box<dyn ConnListener>> {
    match endpoint {
        Endpoint::Tcp(..) | Endpoint::Tls(..) => {}
        _ => return Err(Error::InvalidEndpoint(endpoint.to_string())),
    }

    listen_tls(endpoint.addr()?, endpoint.port()?, config)
        .await
        .map(|l| Box::new(l) as Box<dyn ConnListener>)
}

impl From<TlsStream<TcpStream>> for Box<dyn Connection> {
    fn from(conn: TlsStream<TcpStream>) -> Self {
        Box::new(TlsConn::new(conn.get_ref().0.clone(), conn))
    }
}

impl From<TlsListener> for Box<dyn ConnListener> {
    fn from(listener: TlsListener) -> Self {
        Box::new(listener)
    }
}
