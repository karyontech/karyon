mod peer_conn;

use std::sync::Arc;

use async_trait::async_trait;

use karyon_eventemitter::EventValue;

use crate::{version::Version, Result};

pub use peer_conn::PeerConn;

pub type ProtocolID = String;

/// Protocol event used internally by karyon. User code reads
/// messages via `PeerConn::recv` which yields `Vec<u8>` directly and
/// surfaces shutdown as `Err(PeerShutdown)`.
#[derive(Debug, Clone, EventValue)]
pub enum ProtocolEvent {
    /// Message event, contains a vector of bytes.
    Message(Vec<u8>),
    /// Shutdown event signals the protocol to gracefully shut down.
    Shutdown,
}

/// Whether a protocol is required for handshake / discovery to consider
/// a peer compatible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolKind {
    /// Peers must speak this protocol. Handshake fails if absent.
    /// Discovery filters out peers whose bloom doesn't cover it.
    Mandatory,
    /// Nice-to-have. Discovery prefers peers with overlap but accepts
    /// peers without it. The default for app protocols.
    Optional,
}

/// Per-protocol metadata stored in the peer pool.
#[derive(Clone, Debug)]
pub struct ProtocolMeta {
    pub version: Version,
    pub kind: ProtocolKind,
}

/// The Protocol trait defines the interface for core protocols
/// and custom protocols.
///
/// # Example
/// ```no_run
/// use std::sync::Arc;
///
/// use async_trait::async_trait;
///
/// use karyon_core::async_runtime::global_executor;
/// use karyon_p2p::{
///     protocol::{PeerConn, Protocol, ProtocolID},
///     Node, Config, Version, Error,
///     keypair::{KeyPair, KeyPairType},
/// };
///
/// pub struct NewProtocol {
///     peer: PeerConn,
/// }
///
/// impl NewProtocol {
///     fn new(peer: PeerConn) -> Arc<dyn Protocol> {
///         Arc::new(Self { peer })
///     }
/// }
///
/// #[async_trait]
/// impl Protocol for NewProtocol {
///     async fn start(self: Arc<Self>) -> Result<(), Error> {
///         loop {
///             let bytes = self.peer.recv().await?;
///             println!("{:?}", bytes);
///         }
///     }
///
///     fn version() -> Result<Version, Error> {
///         "0.2.0, >0.1.0".parse()
///     }
///
///     fn id() -> ProtocolID {
///         "NEWPROTOCOLID".into()
///     }
/// }
///
/// async {
///     let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
///     let node = Node::new(&key_pair, Config::default(), global_executor());
///     node.attach_protocol::<NewProtocol>(|peer| Ok(NewProtocol::new(peer))).await.unwrap();
/// };
/// ```
#[async_trait]
pub trait Protocol: Send + Sync {
    /// Drive the protocol to completion. Use `self.peer.recv()` etc.
    async fn start(self: Arc<Self>) -> Result<()>;

    /// Returns the version of the protocol.
    fn version() -> Result<Version>
    where
        Self: Sized;

    /// Returns the unique ProtocolID associated with the protocol.
    fn id() -> ProtocolID
    where
        Self: Sized;

    /// Whether peers must speak this protocol or it's optional.
    /// Defaults to `Optional` -- override for protocols required for
    /// any meaningful interaction (e.g. PING).
    fn kind() -> ProtocolKind
    where
        Self: Sized,
    {
        ProtocolKind::Optional
    }
}

/// User-supplied constructor handed to `Node::attach_protocol`.
/// karyon calls it once per connected peer with a typed `PeerConn`
/// scoped to this protocol. Capture whatever shared state the
/// protocol needs in the closure.
pub type ProtocolConstructor = dyn Fn(PeerConn) -> Result<Arc<dyn Protocol>> + Send + Sync;
