use async_trait::async_trait;

use smol::{
    io::{split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    lock::Mutex,
    net::unix::{UnixListener, UnixStream},
};

use crate::{
    connection::{Connection, ToConn},
    endpoint::Endpoint,
    listener::{ConnListener, ToListener},
    Error, Result,
};

/// Unix domain socket implementation of the [`Connection`] trait.
pub struct UnixConn {
    inner: UnixStream,
    read: Mutex<ReadHalf<UnixStream>>,
    write: Mutex<WriteHalf<UnixStream>>,
}

impl UnixConn {
    /// Creates a new UnixConn
    pub fn new(conn: UnixStream) -> Self {
        let (read, write) = split(conn.clone());
        Self {
            inner: conn,
            read: Mutex::new(read),
            write: Mutex::new(write),
        }
    }
}

#[async_trait]
impl Connection for UnixConn {
    fn peer_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_unix_addr(&self.inner.peer_addr()?))
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_unix_addr(&self.inner.local_addr()?))
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
impl ConnListener for UnixListener {
    fn local_endpoint(&self) -> Result<Endpoint> {
        Ok(Endpoint::new_unix_addr(&self.local_addr()?))
    }

    async fn accept(&self) -> Result<Box<dyn Connection>> {
        let (conn, _) = self.accept().await?;
        Ok(Box::new(UnixConn::new(conn)))
    }
}

/// Connects to the given Unix socket path.
pub async fn dial_unix(path: &String) -> Result<UnixConn> {
    let conn = UnixStream::connect(path).await?;
    Ok(UnixConn::new(conn))
}

/// Listens on the given Unix socket path.
pub fn listen_unix(path: &String) -> Result<UnixListener> {
    let listener = UnixListener::bind(path)?;
    Ok(listener)
}

impl From<UnixStream> for Box<dyn Connection> {
    fn from(conn: UnixStream) -> Self {
        Box::new(UnixConn::new(conn))
    }
}

impl From<UnixListener> for Box<dyn ConnListener> {
    fn from(listener: UnixListener) -> Self {
        Box::new(listener)
    }
}

impl ToConn for UnixStream {
    fn to_conn(self) -> Box<dyn Connection> {
        self.into()
    }
}

impl ToConn for UnixConn {
    fn to_conn(self) -> Box<dyn Connection> {
        Box::new(self)
    }
}

impl ToListener for UnixListener {
    fn to_listener(self) -> Box<dyn ConnListener> {
        self.into()
    }
}
