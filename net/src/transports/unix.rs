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
    Result,
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
impl<C, E> Connection for UnixConn<C>
where
    C: Codec<Error = E> + Clone,
    E: From<std::io::Error>,
{
    type Message = C::Message;
    type Error = E;
    fn peer_endpoint(&self) -> std::result::Result<Endpoint, Self::Error> {
        Ok(self
            .peer_endpoint
            .clone()
            .ok_or(std::io::Error::from(std::io::ErrorKind::AddrNotAvailable))?)
    }

    fn local_endpoint(&self) -> std::result::Result<Endpoint, Self::Error> {
        Ok(self
            .local_endpoint
            .clone()
            .ok_or(std::io::Error::from(std::io::ErrorKind::AddrNotAvailable))?)
    }

    async fn recv(&self) -> std::result::Result<Self::Message, Self::Error> {
        self.read_stream.lock().await.recv().await
    }

    async fn send(&self, msg: Self::Message) -> std::result::Result<(), Self::Error> {
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
impl<C, E> ConnListener for UnixListener<C>
where
    C: Codec<Error = E> + Clone + 'static,
    E: From<std::io::Error>,
{
    type Message = C::Message;
    type Error = E;
    fn local_endpoint(&self) -> std::result::Result<Endpoint, Self::Error> {
        Ok(self.inner.local_addr().and_then(|a| {
            Ok(Endpoint::new_unix_addr(
                a.as_pathname()
                    .ok_or(std::io::ErrorKind::AddrNotAvailable)?,
            ))
        })?)
    }

    async fn accept(&self) -> std::result::Result<Conn<C::Message, E>, Self::Error> {
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

impl<C, E> From<UnixListener<C>> for Listener<C::Message, E>
where
    C: Codec<Error = E> + Clone + 'static,
    E: From<std::io::Error>,
{
    fn from(listener: UnixListener<C>) -> Self {
        Box::new(listener)
    }
}

impl<C, E> ToConn for UnixConn<C>
where
    C: Codec<Error = E> + Clone + 'static,
    E: From<std::io::Error>,
{
    type Message = C::Message;
    type Error = E;
    fn to_conn(self) -> Conn<Self::Message, Self::Error> {
        Box::new(self)
    }
}

impl<C, E> ToListener for UnixListener<C>
where
    C: Codec<Error = E> + Clone + 'static,
    E: From<std::io::Error>,
{
    type Message = C::Message;
    type Error = E;
    fn to_listener(self) -> Listener<Self::Message, Self::Error> {
        Box::new(self)
    }
}
