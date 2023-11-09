use std::collections::HashMap;

use bincode::{Decode, Encode};

use karyons_net::{Addr, Port};

use crate::{protocol::ProtocolID, routing_table::Entry, utils::VersionInt, PeerID};

/// The size of the message header, in bytes.
pub const MSG_HEADER_SIZE: usize = 6;

/// The maximum allowed size for a message in bytes.
pub const MAX_ALLOWED_MSG_SIZE: u32 = 1000000;

/// Defines the main message in the Karyon P2P network.
///
/// This message structure consists of a header and payload, where the header
/// typically contains essential information about the message, and the payload
/// contains the actual data being transmitted.
#[derive(Decode, Encode, Debug, Clone)]
pub struct NetMsg {
    pub header: NetMsgHeader,
    pub payload: Vec<u8>,
}

/// Represents the header of a message.
#[derive(Decode, Encode, Debug, Clone)]
pub struct NetMsgHeader {
    pub command: NetMsgCmd,
    pub payload_size: u32,
}

/// Defines message commands.
#[derive(Decode, Encode, Debug, Clone)]
#[repr(u8)]
pub enum NetMsgCmd {
    Version,
    Verack,
    Protocol,
    Shutdown,

    // NOTE: The following commands are used during the lookup process.
    Ping,
    Pong,
    FindPeer,
    Peer,
    Peers,
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

/// VerAck message acknowledging the receipt of a Version message.
#[derive(Decode, Encode, Debug, Clone)]
pub struct VerAckMsg(pub PeerID);

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

macro_rules! get_msg_payload {
    ($a:ident, $b:expr) => {
        if let NetMsgCmd::$a = $b.header.command {
            $b.payload
        } else {
            return Err(Error::InvalidMsg(format!("Unexpected msg{:?}", $b)));
        }
    };
}

pub(super) use get_msg_payload;

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
