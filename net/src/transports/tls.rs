use std::{net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use futures_rustls::{pki_types, rustls, TlsAcceptor, TlsConnector, TlsStream};
use smol::{
    io::{split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    lock::Mutex,
    net::{TcpListener, TcpStream},
};

use crate::{
    connection::{Connection, ToConn},
    endpoint::Endpoint,
    listener::{ConnListener, ToListener},
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
    endpoint: &Endpoint,
    config: rustls::ClientConfig,
    dns_name: &'static str,
) -> Result<TlsConn> {
    let addr = SocketAddr::try_from(endpoint.clone())?;

    let connector = TlsConnector::from(Arc::new(config));

    let sock = TcpStream::connect(addr).await?;
    sock.set_nodelay(true)?;

    let altname = pki_types::ServerName::try_from(dns_name)?;
    let conn = connector.connect(altname, sock.clone()).await?;
    Ok(TlsConn::new(sock, TlsStream::Client(conn)))
}

/// Connects to the given TLS endpoint, returns `Conn` ([`Connection`]).
pub async fn dial(
    endpoint: &Endpoint,
    config: rustls::ClientConfig,
    dns_name: &'static str,
) -> Result<Box<dyn Connection>> {
    match endpoint {
        Endpoint::Tcp(..) | Endpoint::Tls(..) => {}
        _ => return Err(Error::InvalidEndpoint(endpoint.to_string())),
    }

    dial_tls(endpoint, config, dns_name)
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
pub async fn listen_tls(endpoint: &Endpoint, config: rustls::ServerConfig) -> Result<TlsListener> {
    let addr = SocketAddr::try_from(endpoint.clone())?;
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind(addr).await?;
    Ok(TlsListener { acceptor, listener })
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

impl ToConn for TlsStream<TcpStream> {
    fn to_conn(self) -> Box<dyn Connection> {
        self.into()
    }
}

impl ToConn for TlsConn {
    fn to_conn(self) -> Box<dyn Connection> {
        Box::new(self)
    }
}

impl ToListener for TlsListener {
    fn to_listener(self) -> Box<dyn ConnListener> {
        self.into()
    }
}
