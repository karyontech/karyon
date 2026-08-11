use karyon_net::Endpoint;

use crate::{peer::ConnDirection, protocol::ProtocolID, PeerID};

/// A peer that has completed the handshake but has not joined the pool yet.
pub struct PeerCandidate<'a> {
    pub peer_id: &'a PeerID,
    pub remote_endpoint: &'a Endpoint,
    /// Protocols supported by both sides.
    pub negotiated_protocols: &'a [ProtocolID],
}

/// The party being evaluated.
pub enum Subject<'a> {
    /// Before the handshake. Only the transport address is known.
    Endpoint(&'a Endpoint),
    /// After the handshake. The peer identity is known.
    Peer(&'a PeerCandidate<'a>),
}

/// The purpose of the connection.
#[derive(Clone, Debug)]
pub enum Action {
    /// A peer connection.
    Connect(ConnDirection),
    /// A discovery lookup query.
    Lookup(ConnDirection),
    /// A liveness check. No data is exchanged.
    Probe(ConnDirection),
}

/// Policy that decides which peers this node talks to, and for what.
///
/// Synchronous by design: it runs on the accept loop, so a blocking
/// policy would stall every other inbound connection. Keep its state
/// in memory.
pub trait AccessControl: Send + Sync {
    /// Returns false to drop the connection.
    ///
    /// A `Subject::Endpoint` is evaluated before the handshake, when
    /// only the address is known. A `Subject::Peer` is evaluated after
    /// the handshake, when the identity is available.
    fn allow(&self, subject: &Subject, action: Action) -> bool;
}

/// Default policy. Allows everything.
pub struct AllowAll;

impl AccessControl for AllowAll {
    fn allow(&self, _subject: &Subject, _action: Action) -> bool {
        true
    }
}
