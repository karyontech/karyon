use std::sync::Arc;

use async_trait::async_trait;

use karyon_core::event::EventValue;

use crate::{peer::Peer, version::Version, Result};

pub type ProtocolConstructor = dyn Fn(Arc<Peer>) -> Arc<dyn Protocol> + Send + Sync;

pub type ProtocolID = String;

/// Protocol event
#[derive(Debug, Clone)]
pub enum ProtocolEvent {
    /// Message event, contains a vector of bytes.
    Message(Vec<u8>),
    /// Shutdown event signals the protocol to gracefully shut down.
    Shutdown,
}

impl EventValue for ProtocolEvent {
    fn id() -> &'static str {
        "ProtocolEvent"
    }
}

/// The Protocol trait defines the interface for core protocols
/// and custom protocols.
///
/// # Example
/// ```
/// use std::sync::Arc;
///
/// use async_trait::async_trait;
/// use smol::Executor;
///
/// use karyon_p2p::{
///     protocol::{Protocol, ProtocolID, ProtocolEvent},
///     Backend, PeerID, Config, Version, Error, Peer,
///     keypair::{KeyPair, KeyPairType},
///     };
///
/// pub struct NewProtocol {
///     peer: Arc<Peer>,
/// }
///
/// impl NewProtocol {
///     fn new(peer: Arc<Peer>) -> Arc<dyn Protocol> {
///         Arc::new(Self {
///             peer,
///         })
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
///  async {
///     let key_pair = KeyPair::generate(&KeyPairType::Ed25519);
///     let config = Config::default();
///
///     // Create a new Executor
///     let ex = Arc::new(Executor::new());
///
///     // Create a new Backend
///     let backend = Backend::new(&key_pair, config, ex.into());
///
///     // Attach the NewProtocol
///     let c = move |peer| NewProtocol::new(peer);
///     backend.attach_protocol::<NewProtocol>(c).await.unwrap();
///  };
///
/// ```  
#[async_trait]
pub trait Protocol: Send + Sync {
    /// Start the protocol
    async fn start(self: Arc<Self>) -> Result<()>;

    /// Returns the version of the protocol.
    fn version() -> Result<Version>
    where
        Self: Sized;

    /// Returns the unique ProtocolID associated with the protocol.
    fn id() -> ProtocolID
    where
        Self: Sized;
}

#[async_trait]
pub(crate) trait InitProtocol: Send + Sync {
    type T;
    /// Initialize the protocol
    async fn init(self: Arc<Self>) -> Self::T;
}
