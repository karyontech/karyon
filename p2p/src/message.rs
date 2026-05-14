use bincode::{Decode, Encode};

use karyon_net::{Addr, Endpoint, Port};

use crate::{protocol::ProtocolID, util::encode, Result};

/// Transport protocol used by a peer address.
#[derive(Decode, Encode, Debug, Clone, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Tls,
    Udp,
    Quic,
}

/// A peer address with protocol and priority.
/// Lower priority number means higher preference.
#[derive(Decode, Encode, Debug, Clone, PartialEq, Eq)]
pub struct PeerAddr {
    pub addr: Addr,
    pub port: Port,
    pub protocol: Protocol,
    pub priority: u16,
}

impl PeerAddr {
    /// Convert this PeerAddr to a network Endpoint.
    pub fn to_endpoint(&self) -> Endpoint {
        match self.protocol {
            Protocol::Tcp => Endpoint::Tcp(self.addr.clone(), self.port),
            Protocol::Tls => Endpoint::Tls(self.addr.clone(), self.port),
            Protocol::Udp => Endpoint::Udp(self.addr.clone(), self.port),
            Protocol::Quic => Endpoint::Quic(self.addr.clone(), self.port),
        }
    }

    /// Create a PeerAddr from an Endpoint with a given priority.
    pub fn from_endpoint(endpoint: &Endpoint, priority: u16) -> Option<Self> {
        match endpoint {
            Endpoint::Tcp(addr, port) => Some(PeerAddr {
                addr: addr.clone(),
                port: *port,
                protocol: Protocol::Tcp,
                priority,
            }),
            Endpoint::Tls(addr, port) => Some(PeerAddr {
                addr: addr.clone(),
                port: *port,
                protocol: Protocol::Tls,
                priority,
            }),
            Endpoint::Udp(addr, port) => Some(PeerAddr {
                addr: addr.clone(),
                port: *port,
                protocol: Protocol::Udp,
                priority,
            }),
            Endpoint::Quic(addr, port) => Some(PeerAddr {
                addr: addr.clone(),
                port: *port,
                protocol: Protocol::Quic,
                priority,
            }),
            _ => None,
        }
    }
}

/// Pick the best endpoint from a list of peer addresses that matches
/// one of the supported protocols. Returns the highest-priority (lowest number)
/// matching endpoint.
pub fn pick_endpoint(addrs: &[PeerAddr], supported: &[Protocol]) -> Option<Endpoint> {
    addrs
        .iter()
        .filter(|a| supported.contains(&a.protocol))
        .min_by_key(|a| a.priority)
        .map(|a| a.to_endpoint())
}

/// First message on any new QUIC stream - identifies which protocol it serves.
#[cfg(feature = "quic")]
#[derive(Decode, Encode, Debug, Clone)]
pub struct StreamInit {
    pub protocol_id: ProtocolID,
}

/// Wire envelope for the peer data-plane (the persistent FramedConn
/// between two Nodes after the data-plane handshake).
#[derive(Decode, Encode, Debug, Clone)]
pub struct PeerNetMsg {
    pub header: PeerNetMsgHeader,
    pub payload: Vec<u8>,
}

impl PeerNetMsg {
    pub fn new<T: Encode>(command: PeerNetCmd, t: T) -> Result<Self> {
        Ok(Self {
            header: PeerNetMsgHeader { command },
            payload: encode(&t)?,
        })
    }
}

#[derive(Decode, Encode, Debug, Clone)]
pub struct PeerNetMsgHeader {
    pub command: PeerNetCmd,
}

/// Commands valid on the peer data-plane wire.
#[derive(Decode, Encode, Debug, Clone)]
#[repr(u8)]
pub enum PeerNetCmd {
    /// Initial version-exchange message in the handshake.
    Version,
    /// Acknowledgement for a Version message.
    Verack,
    /// Wraps an application protocol payload (`ProtocolMsg`).
    Protocol,
    /// Graceful close marker.
    Shutdown,
}

/// Wraps an application protocol's payload. Sent inside a `PeerNetMsg`
/// with `PeerNetCmd::Protocol`.
#[derive(Decode, Encode, Debug, Clone)]
pub struct ProtocolMsg {
    pub protocol_id: ProtocolID,
    pub payload: Vec<u8>,
}

/// Graceful close payload. Used by both wire surfaces.
#[derive(Decode, Encode, Debug, Clone)]
pub struct ShutdownMsg(pub u8);
