use bincode::{Decode, Encode};

use karyon_net::{Addr, Port};

/// Specifies the size of the key, in bytes.
pub const KEY_SIZE: usize = 32;

/// An Entry represents a peer in the routing table.
#[derive(Encode, Decode, Clone, Debug)]
pub struct Entry {
    /// The unique key identifying the peer.
    pub key: Key,
    /// The IP address of the peer.
    pub addr: Addr,
    /// TCP port
    pub port: Port,
    /// UDP/TCP port
    pub discovery_port: Port,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        // TODO: this should also compare both addresses (the self.addr == other.addr)
        self.key == other.key
    }
}

/// The unique key identifying the peer.
pub type Key = [u8; KEY_SIZE];

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
