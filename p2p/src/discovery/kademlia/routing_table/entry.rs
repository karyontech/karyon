use bincode::{Decode, Encode};

use crate::{bloom::Bloom, discovery::kademlia::messages::PeerMsg, message::PeerAddr, PeerID};

/// Specifies the size of the key, in bytes.
pub const KEY_SIZE: usize = 32;

/// The unique key identifying the peer.
pub type Key = [u8; KEY_SIZE];

/// An Entry represents a peer in the routing table.
#[derive(Encode, Decode, Clone, Debug)]
pub struct Entry {
    /// The unique key identifying the peer.
    pub key: Key,
    /// Connection addresses for peer-to-peer data, ordered by priority.
    pub addrs: Vec<PeerAddr>,
    /// Discovery addresses for lookup and refresh, ordered by priority.
    pub discovery_addrs: Vec<PeerAddr>,
    /// Bloom filter of protocols the peer claims to support.
    /// Treated as a hint; handshake is the source of truth.
    pub protocols: Bloom,
}

impl Entry {
    /// Returns the primary address (first addr) for subnet checks, if available.
    pub fn primary_addr(&self) -> Option<&karyon_net::Addr> {
        self.addrs.first().map(|a| &a.addr)
    }
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl From<Entry> for PeerMsg {
    fn from(entry: Entry) -> PeerMsg {
        PeerMsg {
            peer_id: PeerID(entry.key),
            addrs: entry.addrs,
            discovery_addrs: entry.discovery_addrs,
            protocols: entry.protocols,
        }
    }
}

impl From<PeerMsg> for Entry {
    fn from(peer: PeerMsg) -> Entry {
        Entry {
            key: peer.peer_id.0,
            addrs: peer.addrs,
            discovery_addrs: peer.discovery_addrs,
            protocols: peer.protocols,
        }
    }
}

/// Calculates the XOR distance between two provided keys.
///
/// The XOR distance is a metric used in Kademlia to measure the closeness
/// of keys.
pub fn xor_distance(key: &Key, other: &Key) -> Key {
    let mut res = [0; 32];
    for (i, (k, o)) in key.iter().zip(other.iter()).enumerate() {
        res[i] = k ^ o;
    }
    res
}
