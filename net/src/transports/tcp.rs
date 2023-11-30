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

/// TCP network connection implementation of the [`Connection`] trait.
pub struct TcpConn {
    inner: TcpStream,
    read: Mutex<ReadHalf<TcpStream>>,
    write: Mutex<WriteHalf<TcpStream>>,
}

impl TcpConn {
    /// Creates a new TcpConn
    pub fn new(conn: TcpStream) -> Self {
        let (read, write) = split(conn.clone());
        Self {
            inner: conn,
            read: Mutex::new(read),
            write: Mutex::new(write),
        }
    }
}

#[async_trait]
impl Connection for TcpConn {
    fn peer_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_tcp_addr(&self.inner.peer_addr()?))
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_tcp_addr(&self.inner.local_addr()?))
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

#[async_trait]
impl ConnListener for TcpListener {
    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_tcp_addr(&self.local_addr()?))
    }

    async fn accept(&self) -> Result<Box<dyn Connection>> {
        let (conn, _) = self.accept().await?;
        conn.set_nodelay(true)?;
        Ok(Box::new(TcpConn::new(conn)))
    }
}

/// Connects to the given TCP address and port.
pub async fn dial_tcp(addr: &Addr, port: &Port) -> Result<TcpConn> {
    let address = format!("{}:{}", addr, port);
    let conn = TcpStream::connect(address).await?;
    conn.set_nodelay(true)?;
    Ok(TcpConn::new(conn))
}

/// Listens on the given TCP address and port.
pub async fn listen_tcp(addr: &Addr, port: &Port) -> Result<TcpListener> {
    let address = format!("{}:{}", addr, port);
    let listener = TcpListener::bind(address).await?;
    Ok(listener)
}

impl From<TcpStream> for Box<dyn Connection> {
    fn from(conn: TcpStream) -> Self {
        Box::new(TcpConn::new(conn))
    }
}

impl From<TcpListener> for Box<dyn ConnListener> {
    fn from(listener: TcpListener) -> Self {
        Box::new(listener)
    }
}
