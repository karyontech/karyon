mod peer_conn;

use std::{
    ops::{BitOr, BitOrAssign},
    sync::Arc,
};

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

/// Bit flags describing how a protocol takes part in handshake and
/// discovery. Combine with `|`. `empty()` means the protocol is
/// negotiated but neither required nor advertised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtocolFlags(u32);

impl ProtocolFlags {
    /// Peers must speak this protocol. Handshake fails if absent and
    /// discovery filters out peers that do not advertise it.
    pub const REQUIRED: Self = Self(1 << 0);
    /// Advertised so discovery prefers peers that also have it, but
    /// peers without it are still accepted. The default.
    pub const PREFERRED: Self = Self(1 << 1);
    /// First bit free for user-defined meaning. Bits below are
    /// reserved by karyon. Kademlia advertises items carrying only
    /// user bits without letting them affect peer selection; custom
    /// `Discovery` impls may give them any meaning.
    pub const USER: Self = Self(1 << 16);

    /// No flags set.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Raw bit pattern.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// True if every bit in `other` is set in `self`.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl From<u32> for ProtocolFlags {
    fn from(bits: u32) -> Self {
        Self(bits)
    }
}

impl BitOr for ProtocolFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for ProtocolFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Per-protocol metadata stored in the peer pool.
#[derive(Clone, Debug)]
pub struct ProtocolMeta {
    pub version: Version,
    pub flags: ProtocolFlags,
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
///     fn new(peer: PeerConn) -> Self {
///         Self { peer }
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
///     node.attach_protocol(NewProtocol::new).await.unwrap();
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

    /// How this protocol takes part in handshake and discovery.
    /// Defaults to `PREFERRED` -- override with `REQUIRED` for
    /// protocols needed for any meaningful interaction (e.g. PING).
    fn flags() -> ProtocolFlags
    where
        Self: Sized,
    {
        ProtocolFlags::PREFERRED
    }
}

/// Boxed protocol constructor stored in the peer pool. Built by
/// `Node::attach_protocol` from the user's `Fn(PeerConn) -> P` closure.
/// karyon calls it once per connected peer with a typed `PeerConn`
/// scoped to this protocol.
pub type ProtocolConstructor = dyn Fn(PeerConn) -> Arc<dyn Protocol> + Send + Sync;
