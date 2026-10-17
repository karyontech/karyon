use std::net::SocketAddr;
#[cfg(any(feature = "tcp", feature = "quic"))]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
#[cfg(any(feature = "tcp", feature = "quic"))]
use std::time::Duration;

use log::{debug, error, info};

use karyon_core::async_runtime::Executor;
#[cfg(any(feature = "tcp", feature = "quic"))]
use karyon_core::async_util::timeout;
use karyon_core::async_util::{TaskGroup, TaskResult};
#[cfg(any(feature = "udp", feature = "tcp", feature = "quic"))]
use karyon_net::Endpoint;

#[cfg(any(feature = "tcp", feature = "quic"))]
use karyon_net::{framed, ByteStream};

#[cfg(feature = "tcp")]
use karyon_net::tcp::TcpListener;

#[cfg(feature = "tls")]
use karyon_net::{
    tls::{ServerTlsConfig, TlsLayer},
    ServerLayer,
};

#[cfg(feature = "quic")]
use karyon_net::{
    quic::{QuicEndpoint, QuicIncoming, ServerQuicConfig},
    Error as NetError, StreamMux,
};

#[cfg(feature = "udp")]
use karyon_net::udp::{listen, UdpConfig, UdpConn};

use crate::attrs::Attribute;
#[cfg(any(feature = "tcp", feature = "quic"))]
use crate::codec::StunCodec;
#[cfg(any(feature = "tcp", feature = "quic"))]
use crate::error::Error;
use crate::error::Result;
use crate::msg::{MessageType, StunMessage};

/// Max time for a TLS or QUIC handshake on an accepted connection.
#[cfg(any(feature = "tls", feature = "quic"))]
const HANDSHAKE_TIMEOUT_SECS: u64 = 5;

/// Max time a stream may sit idle waiting for its next request.
#[cfg(any(feature = "tcp", feature = "quic"))]
const STREAM_IDLE_TIMEOUT_SECS: u64 = 10;

/// Default for `StunConfig::max_connections`.
pub const DEFAULT_MAX_CONNECTIONS: usize = 1024;

/// STUN server configuration. Endpoints left as `None` are not served.
pub struct StunConfig {
    /// UDP listen endpoint (primary), e.g. `udp://0.0.0.0:3478`.
    #[cfg(feature = "udp")]
    pub udp_endpoint: Option<Endpoint>,
    /// TCP listen endpoint, e.g. `tcp://0.0.0.0:3478`.
    #[cfg(feature = "tcp")]
    pub tcp_endpoint: Option<Endpoint>,
    /// TLS listen endpoint and config, e.g. `tls://0.0.0.0:5349`.
    #[cfg(feature = "tls")]
    pub tls_endpoint: Option<(Endpoint, ServerTlsConfig)>,
    /// QUIC listen endpoint and config, e.g. `quic://0.0.0.0:5349`.
    /// Non-standard: STUN over QUIC is a karyon extension, not RFC 5389.
    #[cfg(feature = "quic")]
    pub quic_endpoint: Option<(Endpoint, ServerQuicConfig)>,
    /// Max concurrent TCP, TLS and QUIC connections. Extra connections
    /// are dropped on accept. UDP is not limited. Default
    /// `DEFAULT_MAX_CONNECTIONS`.
    pub max_connections: usize,
    /// Value for the SOFTWARE attribute in responses.
    pub software: Option<String>,
}

impl Default for StunConfig {
    fn default() -> Self {
        Self {
            #[cfg(feature = "udp")]
            udp_endpoint: None,
            #[cfg(feature = "tcp")]
            tcp_endpoint: None,
            #[cfg(feature = "tls")]
            tls_endpoint: None,
            #[cfg(feature = "quic")]
            quic_endpoint: None,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            software: None,
        }
    }
}

/// Stateless STUN server: replies to Binding Requests with the source
/// address it saw them from.
pub struct StunServer {
    config: StunConfig,
    task_group: TaskGroup,
    /// Open TCP, TLS and QUIC connections.
    #[cfg(any(feature = "tcp", feature = "quic"))]
    conns: Arc<AtomicUsize>,
}

/// One taken connection slot. Frees it on drop.
#[cfg(any(feature = "tcp", feature = "quic"))]
struct ConnSlot(Arc<AtomicUsize>);

#[cfg(any(feature = "tcp", feature = "quic"))]
impl Drop for ConnSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl StunServer {
    pub fn new(config: StunConfig, ex: Executor) -> Arc<Self> {
        Arc::new(Self {
            config,
            task_group: TaskGroup::with_executor(ex),
            #[cfg(any(feature = "tcp", feature = "quic"))]
            conns: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// Bind the configured endpoints and start serving. Listeners are
    /// bound before this returns; serving runs in background tasks.
    pub async fn run(self: &Arc<Self>) -> Result<()> {
        #[cfg(feature = "udp")]
        if let Some(endpoint) = &self.config.udp_endpoint {
            let conn = listen(endpoint, UdpConfig::default()).await?;
            info!("STUN listening on {endpoint}");
            self.task_group.spawn_then(
                udp_loop(conn, self.config.software.clone()),
                |res: TaskResult<Result<()>>| async move {
                    debug!("STUN udp task ended: {res}");
                },
            );
        }

        #[cfg(feature = "tcp")]
        if let Some(endpoint) = &self.config.tcp_endpoint {
            let listener = TcpListener::bind(endpoint, Default::default()).await?;
            info!("STUN listening on {endpoint}");
            self.task_group.spawn_then(
                tcp_loop(self.clone(), listener),
                |res: TaskResult<Result<()>>| async move {
                    debug!("STUN tcp task ended: {res}");
                },
            );
        }

        #[cfg(feature = "tls")]
        if let Some((endpoint, config)) = &self.config.tls_endpoint {
            let listener = TcpListener::bind(endpoint, Default::default()).await?;
            let layer = TlsLayer::server(config.clone());
            info!("STUN listening on {endpoint}");
            self.task_group.spawn_then(
                tls_loop(self.clone(), listener, layer),
                |res: TaskResult<Result<()>>| async move {
                    debug!("STUN tls task ended: {res}");
                },
            );
        }

        #[cfg(feature = "quic")]
        if let Some((endpoint, config)) = &self.config.quic_endpoint {
            let quic = QuicEndpoint::listen(endpoint, config.clone()).await?;
            info!("STUN listening on {endpoint}");
            self.task_group.spawn_then(
                quic_loop(self.clone(), quic),
                |res: TaskResult<Result<()>>| async move {
                    debug!("STUN quic task ended: {res}");
                },
            );
        }

        Ok(())
    }

    /// Stop all listeners and per-connection tasks.
    pub async fn shutdown(&self) {
        self.task_group.cancel().await;
    }

    /// Take a connection slot, or None if `max_connections` are open.
    #[cfg(any(feature = "tcp", feature = "quic"))]
    fn take_slot(&self) -> Option<ConnSlot> {
        let open = self.conns.fetch_add(1, Ordering::SeqCst);
        // Dropping the slot on the full path undoes the increment.
        let slot = ConnSlot(self.conns.clone());
        (open < self.config.max_connections).then_some(slot)
    }

    /// Serve one byte stream in its own task. `slot` is held until the
    /// stream ends; QUIC streams pass None since their connection holds it.
    #[cfg(any(feature = "tcp", feature = "quic"))]
    fn spawn_stream(&self, stream: Box<dyn ByteStream>, slot: Option<ConnSlot>) {
        self.task_group.spawn_then(
            stream_conn_loop(stream, self.config.software.clone(), slot),
            |res: TaskResult<Result<()>>| async move {
                debug!("STUN stream conn ended: {res}");
            },
        );
    }
}

/// The whole STUN server logic.
fn handle_binding(msg: &StunMessage, source: SocketAddr, software: &Option<String>) -> StunMessage {
    let mut resp = StunMessage::binding_response(msg, source);
    if let Some(s) = software {
        resp.attrs.push(Attribute::Software(s.clone()));
    }
    resp
}

/// Serve Binding Requests on a UDP socket. One datagram per request.
#[cfg(feature = "udp")]
async fn udp_loop(conn: UdpConn, software: Option<String>) -> Result<()> {
    let mut buf = [0u8; 1500];
    loop {
        // A failed receive (e.g. ICMP unreachable) must not stop the server.
        let (n, source) = match conn.recv_from(&mut buf).await {
            Ok(r) => r,
            Err(e) => {
                error!("STUN: udp receive failed: {e}");
                continue;
            }
        };
        let msg = match StunMessage::decode(&buf[..n]) {
            Ok(Some((_, msg))) => msg,
            Ok(None) => continue,
            Err(e) => {
                debug!("STUN: drop invalid datagram from {source}: {e}");
                continue;
            }
        };
        if msg.msg_type != MessageType::BindingRequest {
            continue;
        }
        // A bad response must not stop the server.
        let bytes = match handle_binding(&msg, source, &software).encode() {
            Ok(b) => b,
            Err(e) => {
                debug!("STUN: encode response for {source}: {e}");
                continue;
            }
        };
        let _ = conn.send_to(&bytes, source).await;
    }
}

/// Accept TCP connections and spawn one task per connection.
#[cfg(feature = "tcp")]
async fn tcp_loop(server: Arc<StunServer>, listener: TcpListener) -> Result<()> {
    loop {
        let stream = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                error!("STUN: tcp accept failed: {e}");
                continue;
            }
        };
        let Some(slot) = server.take_slot() else {
            debug!("STUN: connection limit reached, drop tcp conn");
            continue;
        };
        server.spawn_stream(stream, Some(slot));
    }
}

/// Accept TLS connections. The handshake runs in the per-connection
/// task so a slow peer cannot stall the accept loop.
#[cfg(feature = "tls")]
async fn tls_loop(server: Arc<StunServer>, listener: TcpListener, layer: TlsLayer) -> Result<()> {
    loop {
        let stream = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                error!("STUN: tls accept failed: {e}");
                continue;
            }
        };
        let Some(slot) = server.take_slot() else {
            debug!("STUN: connection limit reached, drop tls conn");
            continue;
        };
        server.task_group.spawn_then(
            tls_conn(server.clone(), layer.clone(), stream, slot),
            |res: TaskResult<Result<()>>| async move {
                debug!("STUN tls conn ended: {res}");
            },
        );
    }
}

/// Run the TLS handshake, then serve the stream.
#[cfg(feature = "tls")]
async fn tls_conn(
    server: Arc<StunServer>,
    layer: TlsLayer,
    stream: Box<dyn ByteStream>,
    slot: ConnSlot,
) -> Result<()> {
    let d = Duration::from_secs(HANDSHAKE_TIMEOUT_SECS);
    let stream = timeout(d, ServerLayer::handshake(&layer, stream))
        .await
        .map_err(|_| Error::Timeout)??;
    stream_conn_loop(stream, server.config.software.clone(), Some(slot)).await
}

/// Accept QUIC connections. The handshake runs in the per-connection
/// task so a slow peer cannot stall the accept loop.
#[cfg(feature = "quic")]
async fn quic_loop(server: Arc<StunServer>, quic: QuicEndpoint) -> Result<()> {
    loop {
        let incoming = match quic.accept_incoming().await {
            Ok(i) => i,
            // The endpoint is closed for good.
            Err(NetError::ConnectionClosed) => return Ok(()),
            Err(e) => {
                error!("STUN: quic accept failed: {e}");
                continue;
            }
        };
        let Some(slot) = server.take_slot() else {
            debug!("STUN: connection limit reached, drop quic conn");
            continue;
        };
        server.task_group.spawn_then(
            quic_conn(server.clone(), incoming, slot),
            |res: TaskResult<Result<()>>| async move {
                debug!("STUN quic conn ended: {res}");
            },
        );
    }
}

/// Run the QUIC handshake, then serve every bidirectional stream the
/// peer opens until the connection closes. Holds `_slot` throughout.
#[cfg(feature = "quic")]
async fn quic_conn(server: Arc<StunServer>, incoming: QuicIncoming, _slot: ConnSlot) -> Result<()> {
    let d = Duration::from_secs(HANDSHAKE_TIMEOUT_SECS);
    let conn = timeout(d, incoming.handshake())
        .await
        .map_err(|_| Error::Timeout)??;
    loop {
        let stream = conn.accept_stream().await?;
        server.spawn_stream(stream, None);
    }
}

/// Serve Binding Requests on one byte stream (TCP, TLS, or a QUIC
/// stream). Stream transports allow multiple requests per connection.
/// The stream closes after `STREAM_IDLE_TIMEOUT_SECS` without a request.
#[cfg(any(feature = "tcp", feature = "quic"))]
async fn stream_conn_loop(
    stream: Box<dyn ByteStream>,
    software: Option<String>,
    _slot: Option<ConnSlot>,
) -> Result<()> {
    let source = stream
        .peer_endpoint()
        .ok_or_else(|| Error::InvalidMsg("connection without peer endpoint".into()))?;
    let source = SocketAddr::try_from(source)?;
    let mut conn = framed(stream, StunCodec);
    let idle = Duration::from_secs(STREAM_IDLE_TIMEOUT_SECS);

    loop {
        let msg = match timeout(idle, conn.recv_msg()).await {
            Ok(Ok(msg)) => msg,
            Ok(Err(e)) => {
                // Peer hung up or sent garbage.
                debug!("STUN: close stream from {source}: {e}");
                return Ok(());
            }
            Err(_) => {
                debug!("STUN: close idle stream from {source}");
                return Ok(());
            }
        };
        if msg.msg_type != MessageType::BindingRequest {
            continue;
        }
        let resp = handle_binding(&msg, source, &software);
        conn.send_msg(resp).await?;
    }
}
