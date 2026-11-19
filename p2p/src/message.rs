use bincode::{Decode, Encode};

use karyon_net::{Addr, Endpoint, Port};

use crate::{Result, protocol::ProtocolID, transport::TransportRef, util::encode};

/// Transport protocol of a built-in peer address.
#[derive(Decode, Encode, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Protocol {
    Tcp,
    Tls,
    Udp,
    Quic,
}

/// A peer address with a priority. Lower priority number means higher
/// preference.
#[derive(Decode, Encode, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PeerAddr {
    /// An IP or domain address on a built-in transport.
    Ip {
        addr: Addr,
        port: Port,
        protocol: Protocol,
        priority: u16,
    },
    /// The address of a user transport, as `scheme://addr`.
    Custom {
        scheme: String,
        addr: String,
        priority: u16,
    },
}

impl PeerAddr {
    /// Convert this PeerAddr to a network Endpoint.
    pub fn to_endpoint(&self) -> Endpoint {
        match self {
            PeerAddr::Ip {
                addr,
                port,
                protocol,
                ..
            } => match protocol {
                Protocol::Tcp => Endpoint::Tcp(addr.clone(), *port),
                Protocol::Tls => Endpoint::Tls(addr.clone(), *port),
                Protocol::Udp => Endpoint::Udp(addr.clone(), *port),
                Protocol::Quic => Endpoint::Quic(addr.clone(), *port),
            },
            PeerAddr::Custom { scheme, addr, .. } => Endpoint::Custom {
                scheme: scheme.clone(),
                addr: addr.clone(),
            },
        }
    }

    /// Create a PeerAddr from an Endpoint with a given priority. `None`
    /// for endpoints peers can not dial (HTTP, WS, Unix).
    pub fn from_endpoint(endpoint: &Endpoint, priority: u16) -> Option<Self> {
        let ip = |addr: &Addr, port: &Port, protocol| PeerAddr::Ip {
            addr: addr.clone(),
            port: *port,
            protocol,
            priority,
        };
        match endpoint {
            Endpoint::Tcp(addr, port) => Some(ip(addr, port, Protocol::Tcp)),
            Endpoint::Tls(addr, port) => Some(ip(addr, port, Protocol::Tls)),
            Endpoint::Udp(addr, port) => Some(ip(addr, port, Protocol::Udp)),
            Endpoint::Quic(addr, port) => Some(ip(addr, port, Protocol::Quic)),
            Endpoint::Custom { scheme, addr } => Some(PeerAddr::Custom {
                scheme: scheme.clone(),
                addr: addr.clone(),
                priority,
            }),
            _ => None,
        }
    }

    pub fn priority(&self) -> u16 {
        match self {
            PeerAddr::Ip { priority, .. } | PeerAddr::Custom { priority, .. } => *priority,
        }
    }

    /// The protocol of a built-in address; `None` for a custom one.
    pub fn protocol(&self) -> Option<&Protocol> {
        match self {
            PeerAddr::Ip { protocol, .. } => Some(protocol),
            PeerAddr::Custom { .. } => None,
        }
    }

    /// The IP or domain of a built-in address; `None` for a custom one.
    pub fn ip_addr(&self) -> Option<&Addr> {
        match self {
            PeerAddr::Ip { addr, .. } => Some(addr),
            PeerAddr::Custom { .. } => None,
        }
    }
}

/// Pick the best endpoint from a list of peer addresses: built-in ones
/// whose protocol is in `supported`, custom ones the `transport` supports.
/// Returns the highest-priority (lowest number) one.
pub fn pick_endpoint(
    addrs: &[PeerAddr],
    supported: &[Protocol],
    transport: Option<&TransportRef>,
) -> Option<Endpoint> {
    addrs
        .iter()
        .filter(|a| match a.protocol() {
            Some(p) => supported.contains(p),
            None => transport.is_some_and(|t| t.supports(&a.to_endpoint())),
        })
        .min_by_key(|a| a.priority())
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::util::decode;

    #[test]
    fn custom_peer_addr_roundtrip() {
        let ep: Endpoint = "mem://node-a".parse().unwrap();
        let pa = PeerAddr::from_endpoint(&ep, 2).unwrap();
        assert_eq!(pa.to_endpoint(), ep);
        assert_eq!(pa.priority(), 2);
        assert!(pa.protocol().is_none());

        let bytes = encode(&pa).unwrap();
        let (decoded, _) = decode::<PeerAddr>(&bytes).unwrap();
        assert_eq!(decoded, pa);
    }

    #[test]
    fn pick_endpoint_skips_custom_without_transport() {
        let addrs = vec![
            PeerAddr::from_endpoint(&"mem://node-a".parse().unwrap(), 0).unwrap(),
            PeerAddr::from_endpoint(&"tcp://10.0.0.1:8000".parse().unwrap(), 1).unwrap(),
        ];
        let ep = pick_endpoint(&addrs, &[Protocol::Tcp], None).unwrap();
        assert_eq!(ep, "tcp://10.0.0.1:8000".parse().unwrap());
    }
}
