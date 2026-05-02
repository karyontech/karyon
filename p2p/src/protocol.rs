use std::sync::Arc;

use async_trait::async_trait;

use karyon_eventemitter::EventValue;

use crate::{peer::Peer, version::Version, Result};

pub type ProtocolConstructor = dyn Fn(Arc<Peer>) -> Arc<dyn Protocol> + Send + Sync;

pub type ProtocolID = String;

/// Protocol event
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
///     protocol::{Protocol, ProtocolID, ProtocolEvent},
///     Node, Config, Version, Error, Peer,
///     keypair::{KeyPair, KeyPairType},
/// };
///
/// pub struct NewProtocol {
///     peer: Arc<Peer>,
/// }
///
/// impl NewProtocol {
///     fn new(peer: Arc<Peer>) -> Arc<dyn Protocol> {
///         Arc::new(Self { peer })
///     }
/// }
///
/// #[async_trait]
/// impl Protocol for NewProtocol {
///     async fn start(self: Arc<Self>) -> Result<(), Error> {
///         loop {
///             match self.peer.recv::<Self>().await.expect("Receive msg") {
///                 ProtocolEvent::Message(msg) => {
///                     println!("{:?}", msg);
///                 }
///                 ProtocolEvent::Shutdown => {
///                     break;
///                 }
///             }
///         }
///         Ok(())
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
///
///     let c = move |peer| NewProtocol::new(peer);
///     node.attach_protocol::<NewProtocol>(c).await.unwrap();
/// };
/// ```
#[async_trait]
pub trait Protocol: Send + Sync {
    /// Start the protocol. Uses `peer.recv::<Self>()` for messages.
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
