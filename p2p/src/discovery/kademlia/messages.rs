//! Wire messages for the Kademlia DHT plane.
//!
//! Two transports:
//! - `KadNetMsg` over a length-prefixed framed TCP/TLS/QUIC connection
//!   (lookup-plane), with `KadNetCmd` as the command tag.
//! - `RefreshMsg` directly over UDP (refresh-plane).

use bincode::{Decode, Encode};

use karyon_net::{
    codec::{Codec, LengthCodec},
    ByteBuffer,
};

use crate::{
    bloom::Bloom,
    message::PeerAddr,
    util::{decode, encode},
    version::VersionInt,
    PeerID, Result,
};

// ---------- Lookup-plane wire envelope ----------

/// Wire envelope for the kademlia lookup-plane (LookupService's own
/// short-lived TCP/TLS/QUIC connections).
#[derive(Decode, Encode, Debug, Clone)]
pub struct KadNetMsg {
    pub header: KadNetMsgHeader,
    pub payload: Vec<u8>,
}

impl KadNetMsg {
    pub fn new<T: Encode>(command: KadNetCmd, t: T) -> Result<Self> {
        Ok(Self {
            header: KadNetMsgHeader { command },
            payload: encode(&t)?,
        })
    }
}

#[derive(Decode, Encode, Debug, Clone)]
pub struct KadNetMsgHeader {
    pub command: KadNetCmd,
}

/// Commands valid on the kademlia lookup-plane wire.
#[derive(Decode, Encode, Debug, Clone)]
#[repr(u8)]
pub enum KadNetCmd {
    /// Lookup-level liveness check with version + nonce.
    Ping,
    /// Reply to Ping carrying the nonce.
    Pong,
    /// Request closest peers to a given peer id.
    FindPeer,
    /// Sender's own PeerMsg (advertise self).
    Peer,
    /// List of PeerMsg returned in response to FindPeer.
    Peers,
    /// Graceful close marker.
    Shutdown,
}

/// Length-prefixed bincode codec for the kademlia lookup-plane wire.
#[derive(Clone)]
pub struct KadNetMsgCodec {
    inner_codec: LengthCodec,
}

impl KadNetMsgCodec {
    pub fn new() -> Self {
        Self {
            inner_codec: LengthCodec::default(),
        }
    }
}

impl Default for KadNetMsgCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Codec<ByteBuffer> for KadNetMsgCodec {
    type Message = KadNetMsg;
    type Error = karyon_net::Error;

    fn encode(
        &self,
        src: &KadNetMsg,
        dst: &mut ByteBuffer,
    ) -> std::result::Result<usize, karyon_net::Error> {
        let src = encode(src).map_err(|e| karyon_net::Error::IO(std::io::Error::other(e)))?;
        self.inner_codec.encode(&src, dst)
    }

    fn decode(
        &self,
        src: &mut ByteBuffer,
    ) -> std::result::Result<Option<(usize, KadNetMsg)>, karyon_net::Error> {
        match self.inner_codec.decode(src)? {
            Some((n, s)) => {
                let (m, _) = decode::<KadNetMsg>(&s)
                    .map_err(|e| karyon_net::Error::IO(std::io::Error::other(e)))?;
                Ok(Some((n, m)))
            }
            None => Ok(None),
        }
    }
}

// ---------- Lookup-plane payloads ----------

/// Ping payload: liveness + version probe.
#[derive(Decode, Encode, Debug, Clone)]
pub struct PingMsg {
    pub nonce: [u8; 32],
    pub version: VersionInt,
}

/// Pong payload echoing back the nonce.
#[derive(Decode, Encode, Debug)]
pub struct PongMsg(pub [u8; 32]);

/// FindPeer payload: target peer id we want closest peers for.
#[derive(Decode, Encode, Debug)]
pub struct FindPeerMsg(pub PeerID);

/// Peer payload: a single peer's advertised addresses + protocol bloom.
#[derive(Decode, Encode, Debug, Clone, PartialEq, Eq)]
pub struct PeerMsg {
    pub peer_id: PeerID,
    pub addrs: Vec<PeerAddr>,
    pub discovery_addrs: Vec<PeerAddr>,
    pub protocols: Bloom,
}

/// Peers payload: list of peer entries returned by FindPeer.
#[derive(Decode, Encode, Debug)]
pub struct PeersMsg {
    pub peers: Vec<PeerMsg>,
}

// ---------- Refresh-plane wire (UDP) ----------

/// Refresh-plane message sent directly over UDP. No envelope/codec
/// because each datagram carries one self-describing message.
#[derive(Decode, Encode, Debug, Clone)]
pub enum RefreshMsg {
    Ping([u8; 32]),
    Pong([u8; 32]),
}
