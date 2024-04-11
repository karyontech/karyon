use async_trait::async_trait;
use futures_util::SinkExt;

use karyon_core::async_runtime::{
    io::{split, ReadHalf, WriteHalf},
    lock::Mutex,
    net::{UnixListener as AsyncUnixListener, UnixStream},
};

use crate::{
    codec::Codec,
    connection::{Conn, Connection, ToConn},
    endpoint::Endpoint,
    listener::{ConnListener, Listener, ToListener},
    stream::{ReadStream, WriteStream},
    Error, Result,
};

/// Unix Conn config
#[derive(Clone, Default)]
pub struct UnixConfig {}

/// Unix domain socket implementation of the [`Connection`] trait.
pub struct UnixConn<C> {
    read_stream: Mutex<ReadStream<ReadHalf<UnixStream>, C>>,
    write_stream: Mutex<WriteStream<WriteHalf<UnixStream>, C>>,
    peer_endpoint: Option<Endpoint>,
    local_endpoint: Option<Endpoint>,
}

impl<C> UnixConn<C>
where
    C: Codec + Clone,
{
    /// Creates a new TcpConn
    pub fn new(conn: UnixStream, codec: C) -> Self {
        let peer_endpoint = conn
            .peer_addr()
            .and_then(|a| {
                Ok(Endpoint::new_unix_addr(
                    a.as_pathname()
                        .ok_or(std::io::ErrorKind::AddrNotAvailable)?,
                ))
            })
            .ok();
        let local_endpoint = conn
            .local_addr()
            .and_then(|a| {
                Ok(Endpoint::new_unix_addr(
                    a.as_pathname()
                        .ok_or(std::io::ErrorKind::AddrNotAvailable)?,
                ))
            })
            .ok();

        let (read, write) = split(conn);
        let read_stream = Mutex::new(ReadStream::new(read, codec.clone()));
        let write_stream = Mutex::new(WriteStream::new(write, codec));
        Self {
            read_stream,
            write_stream,
            peer_endpoint,
            local_endpoint,
        }
    }
}

#[async_trait]
impl<C> Connection for UnixConn<C>
where
    C: Codec + Clone,
{
    type Item = C::Item;
    fn peer_endpoint(&self) -> Result<Endpoint> {
        self.peer_endpoint
            .clone()
            .ok_or(Error::IO(std::io::ErrorKind::AddrNotAvailable.into()))
    }

    fn local_endpoint(&self) -> Result<Endpoint> {
        self.local_endpoint
            .clone()
            .ok_or(Error::IO(std::io::ErrorKind::AddrNotAvailable.into()))
    }

    async fn recv(&self) -> Result<Self::Item> {
        self.read_stream.lock().await.recv().await
    }

    async fn send(&self, msg: Self::Item) -> Result<()> {
        self.write_stream.lock().await.send(msg).await
    }
}

#[allow(dead_code)]
pub struct UnixListener<C> {
    inner: AsyncUnixListener,
    config: UnixConfig,
    codec: C,
}

impl<C> UnixListener<C>
where
    C: Codec + Clone,
{
    pub fn new(listener: AsyncUnixListener, config: UnixConfig, codec: C) -> Self {
        Self {
            inner: listener,
            config,
            codec,
        }
    }
}

#[async_trait]
impl<C> ConnListener for UnixListener<C>
where
    C: Codec + Clone,
{
    type Item = C::Item;
    fn local_endpoint(&self) -> Result<Endpoint> {
        self.inner
            .local_addr()
            .and_then(|a| {
                Ok(Endpoint::new_unix_addr(
                    a.as_pathname()
                        .ok_or(std::io::ErrorKind::AddrNotAvailable)?,
                ))
            })
            .map_err(Error::from)
    }

    async fn accept(&self) -> Result<Conn<C::Item>> {
        let (conn, _) = self.inner.accept().await?;
        Ok(Box::new(UnixConn::new(conn, self.codec.clone())))
    }
}

/// Connects to the given Unix socket path.
pub async fn dial<C>(endpoint: &Endpoint, _config: UnixConfig, codec: C) -> Result<UnixConn<C>>
where
    C: Codec + Clone,
{
    let path: std::path::PathBuf = endpoint.clone().try_into()?;
    let conn = UnixStream::connect(path).await?;
    Ok(UnixConn::new(conn, codec))
}

/// Listens on the given Unix socket path.
pub fn listen<C>(endpoint: &Endpoint, config: UnixConfig, codec: C) -> Result<UnixListener<C>>
where
    C: Codec + Clone,
{
    let path: std::path::PathBuf = endpoint.clone().try_into()?;
    let listener = AsyncUnixListener::bind(path)?;
    Ok(UnixListener::new(listener, config, codec))
}

// impl From<UnixStream> for Box<dyn Connection> {
//     fn from(conn: UnixStream) -> Self {
//         Box::new(UnixConn::new(conn))
//     }
// }

impl<C> From<UnixListener<C>> for Listener<C::Item>
where
    C: Codec + Clone,
{
    fn from(listener: UnixListener<C>) -> Self {
        Box::new(listener)
    }
}

impl<C> ToConn for UnixConn<C>
where
    C: Codec + Clone,
{
    type Item = C::Item;
    fn to_conn(self) -> Conn<Self::Item> {
        Box::new(self)
    }
}

impl<C> ToListener for UnixListener<C>
where
    C: Codec + Clone,
{
    type Item = C::Item;
    fn to_listener(self) -> Listener<Self::Item> {
        self.into()
    }
}
