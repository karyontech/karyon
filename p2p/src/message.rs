use std::collections::HashMap;

use bincode::{Decode, Encode};

use karyon_core::util::encode;
use karyon_net::{Addr, Port};

use crate::{protocol::ProtocolID, routing_table::Entry, version::VersionInt, PeerID, Result};

/// Defines the main message in the karyon p2p network.
///
/// This message structure consists of a header and payload, where the header
/// typically contains essential information about the message, and the payload
/// contains the actual data being transmitted.
#[derive(Decode, Encode, Debug, Clone)]
pub struct NetMsg {
    pub header: NetMsgHeader,
    pub payload: Vec<u8>,
}

impl NetMsg {
    pub fn new<T: Encode>(command: NetMsgCmd, t: T) -> Result<Self> {
        Ok(Self {
            header: NetMsgHeader { command },
            payload: encode(&t)?,
        })
    }
}

/// Represents the header of a message.
#[derive(Decode, Encode, Debug, Clone)]
pub struct NetMsgHeader {
    pub command: NetMsgCmd,
}

/// Defines message commands.
#[derive(Decode, Encode, Debug, Clone)]
#[repr(u8)]
pub enum NetMsgCmd {
    Version,
    Verack,
    Protocol,
    Shutdown,

    // The following commands are used during the lookup process.
    Ping,
    Pong,
    FindPeer,
    Peer,
    Peers,
}

#[derive(Decode, Encode, Debug, Clone)]
pub enum RefreshMsg {
    Ping([u8; 32]),
    Pong([u8; 32]),
}

/// Defines a message related to a specific protocol.
#[derive(Decode, Encode, Debug, Clone)]
pub struct ProtocolMsg {
    pub protocol_id: ProtocolID,
    pub payload: Vec<u8>,
}

/// Version message, providing information about a peer's capabilities.
#[derive(Decode, Encode, Debug, Clone)]
pub struct VerMsg {
    pub peer_id: PeerID,
    pub version: VersionInt,
    pub protocols: HashMap<ProtocolID, VersionInt>,
}

/// VerAck message acknowledges the receipt of a Version message. The message
/// consists of the peer ID and an acknowledgment boolean value indicating
/// whether the version is accepted.
#[derive(Decode, Encode, Debug, Clone)]
pub struct VerAckMsg {
    pub peer_id: PeerID,
    pub ack: bool,
}

/// Shutdown message.
#[derive(Decode, Encode, Debug, Clone)]
pub struct ShutdownMsg(pub u8);

/// Ping message with a nonce and version information.
#[derive(Decode, Encode, Debug, Clone)]
pub struct PingMsg {
    pub nonce: [u8; 32],
    pub version: VersionInt,
}

/// Ping message with a nonce.
#[derive(Decode, Encode, Debug)]
pub struct PongMsg(pub [u8; 32]);

/// FindPeer message used to find a specific peer.
#[derive(Decode, Encode, Debug)]
pub struct FindPeerMsg(pub PeerID);

/// PeerMsg containing information about a peer.
#[derive(Decode, Encode, Debug, Clone, PartialEq, Eq)]
pub struct PeerMsg {
    pub peer_id: PeerID,
    pub addr: Addr,
    pub port: Port,
    pub discovery_port: Port,
}

/// PeersMsg a list of `PeerMsg`.
#[derive(Decode, Encode, Debug)]
pub struct PeersMsg(pub Vec<PeerMsg>);

impl From<Entry> for PeerMsg {
    fn from(entry: Entry) -> PeerMsg {
        PeerMsg {
            peer_id: PeerID(entry.key),
            addr: entry.addr,
            port: entry.port,
            discovery_port: entry.discovery_port,
        }
    }
}

impl From<PeerMsg> for Entry {
    fn from(peer: PeerMsg) -> Entry {
        Entry {
            key: peer.peer_id.0,
            addr: peer.addr,
            port: peer.port,
            discovery_port: peer.discovery_port,
        }
    }
}

macro_rules! get_msg_payload {
    ($a:ident, $b:ident) => {
        if let NetMsgCmd::$a = $b.header.command {
            $b.payload
        } else {
            return Err(Error::InvalidMsg(format!(
                "Expected {:?} msg found {:?} msg",
                stringify!($a),
                $b.header.command
            )));
        }
    };
}

pub(super) use get_msg_payload;
