use std::sync::Arc;

use async_trait::async_trait;

use karyon_core::event::EventValue;

use crate::{peer::ArcPeer, version::Version, Result};

pub type ArcProtocol = Arc<dyn Protocol>;

pub type ProtocolConstructor = dyn Fn(ArcPeer) -> Arc<dyn Protocol> + Send + Sync;

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
///     protocol::{ArcProtocol, Protocol, ProtocolID, ProtocolEvent},
///     Backend, PeerID, Config, Version, P2pError, ArcPeer,
///     keypair::{KeyPair, KeyPairType},
///     };
///
/// pub struct NewProtocol {
///     peer: ArcPeer,
/// }
///
/// impl NewProtocol {
///     fn new(peer: ArcPeer) -> ArcProtocol {
///         Arc::new(Self {
///             peer,
///         })
///     }
/// }
///
/// #[async_trait]
/// impl Protocol for NewProtocol {
///     async fn start(self: Arc<Self>) -> Result<(), P2pError> {
///         let listener = self.peer.register_listener::<Self>().await;
///         loop {
///             let event = listener.recv().await.unwrap();
///
///             match event {
///                 ProtocolEvent::Message(msg) => {
///                     println!("{:?}", msg);
///                 }
///                 ProtocolEvent::Shutdown => {
///                     break;
///                 }
///             }
///         }
///
///         listener.cancel().await;
///         Ok(())
///     }
///
///     fn version() -> Result<Version, P2pError> {
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
