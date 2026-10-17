use std::net::SocketAddr;
use std::time::Duration;
#[cfg(feature = "udp")]
use std::time::Instant;

#[cfg(any(feature = "tcp", feature = "quic"))]
use std::future::Future;

#[cfg(feature = "udp")]
use log::debug;

use karyon_core::async_util::timeout;
use karyon_net::Endpoint;

#[cfg(any(feature = "tcp", feature = "quic"))]
use karyon_net::{framed, ByteStream};

#[cfg(feature = "tcp")]
use karyon_net::tcp::connect;

#[cfg(feature = "tls")]
use karyon_net::{
    tls::{ClientTlsConfig, TlsLayer},
    ClientLayer,
};

#[cfg(feature = "quic")]
use karyon_net::{
    quic::{ClientQuicConfig, QuicEndpoint},
    StreamMux,
};

#[cfg(feature = "udp")]
use karyon_net::udp::{listen, UdpConfig, UdpConn};

use crate::attrs::Attribute;
use crate::error::{Error, Result};
use crate::msg::{MessageType, StunMessage};

#[cfg(any(feature = "tcp", feature = "quic"))]
use crate::codec::StunCodec;

/// UDP retransmit schedule in milliseconds (RFC 5389 7.2.1, shortened).
#[cfg(feature = "udp")]
const UDP_RETRIES_MS: [u64; 3] = [500, 1000, 2000];

/// Max time for a whole query over a stream transport, including the
/// TLS or QUIC handshake.
#[cfg(any(feature = "tcp", feature = "quic"))]
const STREAM_TIMEOUT_SECS: u64 = 5;

/// STUN binding client. Asks a STUN server for this host's public
/// address as seen from outside the NAT.
///
/// The server endpoint must use an IP address (`udp://`, `tcp://`,
/// `tls://` or `quic://`); hostname resolution is up to the caller.
/// TLS and QUIC need a client config, see `with_tls` and `with_quic`.
pub struct StunClient {
    server: Endpoint,
    #[cfg(feature = "tls")]
    tls: Option<ClientTlsConfig>,
    #[cfg(feature = "quic")]
    quic: Option<ClientQuicConfig>,
}

impl StunClient {
    pub fn new(server: Endpoint) -> Self {
        Self {
            server,
            #[cfg(feature = "tls")]
            tls: None,
            #[cfg(feature = "quic")]
            quic: None,
        }
    }

    /// Set the TLS config used for `tls://` servers.
    #[cfg(feature = "tls")]
    pub fn with_tls(mut self, config: ClientTlsConfig) -> Self {
        self.tls = Some(config);
        self
    }

    /// Set the QUIC config used for `quic://` servers. Non-standard:
    /// only karyon STUN servers speak STUN over QUIC.
    #[cfg(feature = "quic")]
    pub fn with_quic(mut self, config: ClientQuicConfig) -> Self {
        self.quic = Some(config);
        self
    }

    /// Send a Binding Request and return our public address.
    pub async fn discover(&self) -> Result<SocketAddr> {
        match &self.server {
            #[cfg(feature = "udp")]
            Endpoint::Udp(..) => self.discover_udp(None).await,
            #[cfg(feature = "tcp")]
            Endpoint::Tcp(..) => with_timeout(self.discover_tcp()).await,
            #[cfg(feature = "tls")]
            Endpoint::Tls(..) => with_timeout(self.discover_tls()).await,
            #[cfg(feature = "quic")]
            Endpoint::Quic(..) => with_timeout(self.discover_quic()).await,
            ep => Err(Error::UnsupportedEndpoint(ep.to_string())),
        }
    }

    /// Like `discover`, but bind the query socket to `local`, to learn
    /// the public mapping of that specific socket. UDP only.
    #[cfg(feature = "udp")]
    pub async fn discover_from(&self, local: SocketAddr) -> Result<SocketAddr> {
        self.discover_udp(Some(local)).await
    }

    #[cfg(feature = "udp")]
    async fn discover_udp(&self, local: Option<SocketAddr>) -> Result<SocketAddr> {
        let server = SocketAddr::try_from(self.server.clone())?;
        let local = local.unwrap_or_else(|| match server {
            SocketAddr::V4(_) => "0.0.0.0:0".parse().expect("valid address"),
            SocketAddr::V6(_) => "[::]:0".parse().expect("valid address"),
        });

        let conn = listen(&Endpoint::new_udp_addr(local), UdpConfig::default()).await?;
        let request = StunMessage::binding_request();
        let encoded = request.encode()?;

        for ms in UDP_RETRIES_MS {
            conn.send_to(&encoded, server).await?;
            let window = Duration::from_millis(ms);
            if let Some(res) = await_udp_response(&conn, server, &request, window).await {
                return res;
            }
            debug!("STUN: no response within {ms}ms, retrying");
        }
        Err(Error::Timeout)
    }

    #[cfg(feature = "tcp")]
    async fn discover_tcp(&self) -> Result<SocketAddr> {
        let stream = connect(&self.server, Default::default()).await?;
        query_stream(stream).await
    }

    #[cfg(feature = "tls")]
    async fn discover_tls(&self) -> Result<SocketAddr> {
        let config = self.tls.clone().ok_or(Error::MissingConfig("tls"))?;
        let stream = connect(&self.server, Default::default()).await?;
        let stream = ClientLayer::handshake(&TlsLayer::client(config), stream).await?;
        query_stream(stream).await
    }

    #[cfg(feature = "quic")]
    async fn discover_quic(&self) -> Result<SocketAddr> {
        let config = self.quic.clone().ok_or(Error::MissingConfig("quic"))?;
        let conn = QuicEndpoint::dial(&self.server, config).await?;
        let stream = conn.open_stream().await?;
        let res = query_stream(stream).await;
        conn.close(0, b"done");
        res
    }
}

/// Wait up to `window` for the response to `request` from `server`.
/// Stray, garbage, and mismatched datagrams are skipped without ending
/// the window. Returns None when the window ends.
#[cfg(feature = "udp")]
async fn await_udp_response(
    conn: &UdpConn,
    server: SocketAddr,
    request: &StunMessage,
    window: Duration,
) -> Option<Result<SocketAddr>> {
    let deadline = Instant::now() + window;
    let mut buf = [0u8; 1500];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let (n, from) = match timeout(left, conn.recv_from(&mut buf)).await {
            Ok(Ok(received)) => received,
            Ok(Err(e)) => return Some(Err(e.into())),
            Err(_) => return None,
        };
        if from != server {
            continue;
        }
        let msg = match StunMessage::decode(&buf[..n]) {
            Ok(Some((_, msg))) => msg,
            _ => continue,
        };
        match validate_response(request, msg) {
            Err(Error::TransactionMismatch) => continue,
            res => return Some(res),
        }
    }
}

/// Bound a stream query by `STREAM_TIMEOUT_SECS`.
#[cfg(any(feature = "tcp", feature = "quic"))]
async fn with_timeout<F>(fut: F) -> Result<SocketAddr>
where
    F: Future<Output = Result<SocketAddr>>,
{
    let d = Duration::from_secs(STREAM_TIMEOUT_SECS);
    timeout(d, fut).await.map_err(|_| Error::Timeout)?
}

/// Send one Binding Request on a byte stream and read the response.
#[cfg(any(feature = "tcp", feature = "quic"))]
async fn query_stream(stream: Box<dyn ByteStream>) -> Result<SocketAddr> {
    let mut conn = framed(stream, StunCodec);
    let request = StunMessage::binding_request();
    conn.send_msg(request.clone()).await?;
    let msg = conn.recv_msg().await?;
    validate_response(&request, msg)
}

/// Check a response against its request and extract the mapped address.
fn validate_response(request: &StunMessage, response: StunMessage) -> Result<SocketAddr> {
    if response.txn_id != request.txn_id {
        return Err(Error::TransactionMismatch);
    }
    match response.msg_type {
        MessageType::BindingResponse => response.mapped_address(),
        MessageType::BindingError => {
            for attr in &response.attrs {
                if let Attribute::ErrorCode { code, reason } = attr {
                    return Err(Error::ErrorResponse {
                        code: *code,
                        reason: reason.clone(),
                    });
                }
            }
            Err(Error::ErrorResponse {
                code: 0,
                reason: "error response without ERROR-CODE".into(),
            })
        }
        MessageType::BindingRequest => Err(Error::InvalidMsg("request sent as response".into())),
    }
}
