use std::net::SocketAddr;

use async_trait::async_trait;
use smol::{
    io::{split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    lock::Mutex,
    net::{TcpListener, TcpStream},
};

use ws_stream_tungstenite::WsStream;

use crate::{
    connection::{Connection, ToConn},
    endpoint::Endpoint,
    listener::{ConnListener, ToListener},
    Error, Result,
};

/// WS network connection implementation of the [`Connection`] trait.
pub struct WsConn {
    inner: TcpStream,
    read: Mutex<ReadHalf<WsStream<TcpStream>>>,
    write: Mutex<WriteHalf<WsStream<TcpStream>>>,
}

impl WsConn {
    /// Creates a new WsConn
    pub fn new(inner: TcpStream, conn: WsStream<TcpStream>) -> Self {
        let (read, write) = split(conn);
        Self {
            inner,
            read: Mutex::new(read),
            write: Mutex::new(write),
        }
    }
}

#[async_trait]
impl Connection for WsConn {
    fn peer_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_ws_addr(&self.inner.peer_addr()?))
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_ws_addr(&self.inner.local_addr()?))
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

/// Ws network listener implementation of the `Listener` [`ConnListener`] trait.
pub struct WsListener {
    inner: TcpListener,
}

#[async_trait]
impl ConnListener for WsListener {
    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_ws_addr(&self.inner.local_addr()?))
    }

    async fn accept(&self) -> Result<Box<dyn Connection>> {
        let (stream, _) = self.inner.accept().await?;
        let conn = async_tungstenite::accept_async(stream.clone()).await?;
        Ok(Box::new(WsConn::new(stream, WsStream::new(conn))))
    }
}

/// Connects to the given WS address and port.
pub async fn dial(endpoint: &Endpoint) -> Result<WsConn> {
    let addr = SocketAddr::try_from(endpoint.clone())?;
    let stream = TcpStream::connect(addr).await?;
    let (conn, _resp) =
        async_tungstenite::client_async(endpoint.to_string(), stream.clone()).await?;
    Ok(WsConn::new(stream, WsStream::new(conn)))
}

/// Listens on the given WS address and port.
pub async fn listen(endpoint: &Endpoint) -> Result<WsListener> {
    let addr = SocketAddr::try_from(endpoint.clone())?;
    let listener = TcpListener::bind(addr).await?;
    Ok(WsListener { inner: listener })
}

impl From<WsListener> for Box<dyn ConnListener> {
    fn from(listener: WsListener) -> Self {
        Box::new(listener)
    }
}

impl ToConn for WsConn {
    fn to_conn(self) -> Box<dyn Connection> {
        Box::new(self)
    }
}

impl ToListener for WsListener {
    fn to_listener(self) -> Box<dyn ConnListener> {
        self.into()
    }
}
