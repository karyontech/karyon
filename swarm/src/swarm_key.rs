use sha2::{Digest, Sha256};

/// A 32-byte key identifying a swarm.
pub type SwarmKey = [u8; 32];

/// Default swarm key: `sha256(protocol_id)`. One swarm per protocol.
pub fn swarm_key_from_protocol(protocol_id: &str) -> SwarmKey {
    let mut hasher = Sha256::new();
    hasher.update(protocol_id.as_bytes());
    hasher.finalize().into()
}

/// Swarm key with an extra instance name for sub-grouping within a
/// protocol: `sha256(protocol_id + ":" + instance)`.
pub fn compute_swarm_key(protocol_id: &str, instance: &str) -> SwarmKey {
    let mut hasher = Sha256::new();
    hasher.update(protocol_id.as_bytes());
    hasher.update(b":");
    hasher.update(instance.as_bytes());
    hasher.finalize().into()
}
