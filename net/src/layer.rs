use std::future::Future;

use crate::Result;

/// Client-side middleware layer. Upgrades an input via handshake.
///
/// Generic over input and output types:
/// - `ClientLayer<Box<dyn ByteStream>, Box<dyn ByteStream>>` - TLS, SOCKS5, Noise
/// - `ClientLayer<Box<dyn ByteStream>, WsConn<C>>` - WebSocket
///
/// # Example
///
/// ```no_run
/// use karyon_net::{tcp, tls, ClientLayer, ByteStream, Endpoint};
/// use karyon_net::tls::ClientTlsConfig;
///
/// async {
///     let ep: Endpoint = "tcp://127.0.0.1:443".parse().unwrap();
///     let stream = tcp::connect(&ep, Default::default()).await.unwrap();
///     // let tls_stream: Box<dyn ByteStream> = ClientLayer::handshake(
///     //     &tls::TlsLayer::client(config), stream
///     // ).await.unwrap();
/// };
/// ```
pub trait ClientLayer<In, Out>: Send + Sync + Clone {
    fn handshake(&self, input: In) -> impl Future<Output = Result<Out>> + Send;
}

/// Server-side middleware layer. Upgrades an accepted input via handshake.
///
/// Same generic pattern as `ClientLayer`.
pub trait ServerLayer<In, Out>: Send + Sync + Clone {
    fn handshake(&self, input: In) -> impl Future<Output = Result<Out>> + Send;
}
