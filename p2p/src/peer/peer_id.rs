use base64::{engine::general_purpose::STANDARD, Engine};
use bincode::{Decode, Encode};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use karyon_core::crypto::PublicKey;

use crate::Error;

/// Represents a unique identifier for a peer.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Decode, Encode)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(into = "String"))]
pub struct PeerID(pub [u8; 32]);

impl std::fmt::Display for PeerID {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let id = STANDARD.encode(self.0);
        write!(f, "{id}")
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

impl From<PeerID> for String {
    fn from(pid: PeerID) -> Self {
        pid.to_string()
    }
}

impl TryFrom<String> for PeerID {
    type Error = Error;

    fn try_from(i: String) -> Result<Self, Self::Error> {
        let result: [u8; 32] = STANDARD
            .decode(i)?
            .try_into()
            .map_err(|_| Error::PeerIDTryFromString)?;
        Ok(PeerID(result))
    }
}

impl TryFrom<PublicKey> for PeerID {
    type Error = Error;

    fn try_from(pk: PublicKey) -> Result<Self, Self::Error> {
        let pk: [u8; 32] = pk
            .as_bytes()
            .try_into()
            .map_err(|_| Error::PeerIDTryFromPublicKey)?;

        Ok(PeerID(pk))
    }
}
