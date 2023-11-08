use bincode::{Decode, Encode};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

/// Represents a unique identifier for a peer.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Decode, Encode)]
pub struct PeerID(pub [u8; 32]);

impl std::fmt::Display for PeerID {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let id = self.0[0..8]
            .iter()
            .map(|b| format!("{:x}", b))
            .collect::<Vec<String>>()
            .join("");

        write!(f, "{}", id)
    }
}

impl PeerID {
    /// Creates a new PeerID.
    pub fn new(src: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(src);
        Self(hasher.finalize().into())
    }

    /// Generates a random PeerID.
    pub fn random() -> Self {
        let mut id: [u8; 32] = [0; 32];
        OsRng.fill_bytes(&mut id);
        Self(id)
    }
}

impl From<[u8; 32]> for PeerID {
    fn from(b: [u8; 32]) -> Self {
        PeerID(b)
    }
}
