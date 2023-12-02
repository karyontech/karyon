use async_trait::async_trait;

use crate::{
    transports::{tcp, unix},
    Conn, Endpoint, Error, Result,
};

/// Alias for `Box<dyn ConnListener>`
pub type Listener = Box<dyn ConnListener>;

/// A trait for objects which can be converted to [`Listener`].
pub trait ToListener {
    fn to_listener(self) -> Listener;
}

/// ConnListener is a generic network listener.
#[async_trait]
pub trait ConnListener: Send + Sync {
    fn local_endpoint(&self) -> Result<Endpoint>;
    async fn accept(&self) -> Result<Conn>;
}

/// Listens to the provided endpoint.
///
/// it only supports `tcp4/6`, and `unix`.
///
/// #Example
///
/// ```
/// use karyon_net::{Endpoint, listen};
///
/// async {
///     let endpoint: Endpoint = "tcp://127.0.0.1:3000".parse().unwrap();
///
///     let listener = listen(&endpoint).await.unwrap();
///     let conn = listener.accept().await.unwrap();
/// };
///
/// ```
pub async fn listen(endpoint: &Endpoint) -> Result<Box<dyn ConnListener>> {
    match endpoint {
        Endpoint::Tcp(addr, port) => Ok(Box::new(tcp::listen_tcp(addr, port).await?)),
        Endpoint::Unix(addr) => Ok(Box::new(unix::listen_unix(addr)?)),
        _ => Err(Error::InvalidEndpoint(endpoint.to_string())),
    }
}
