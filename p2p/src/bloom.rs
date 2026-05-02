use std::{hash::Hasher, sync::Arc};

use bincode::{Decode, Encode};
use parking_lot::RwLock;
use siphasher::sip::SipHasher13;

/// Shared handle to the local bloom. Readers snapshot via `*b.read()`;
/// writers (`attach_protocol`, swarm joins, ...) update in place. Cheap
/// to clone (Arc) and lock-free for short reads (parking_lot RwLock).
pub type BloomRef = Arc<RwLock<Bloom>>;

/// Two 128-bit bloom filters (k=2 hashes) bundled into one wire type.
///
/// `mandatory` holds items the local node REQUIRES peers to also support.
///
/// `optional` holds items the local node would LIKE peers to support
/// but doesn't require.
///
/// Content-agnostic: items can be protocol ids, swarm keys, or any
/// other identifier hashable as bytes. Discovery-layer hint, not
#[derive(Encode, Decode, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bloom {
    pub mandatory: u128,
    pub optional: u128,
}

impl Bloom {
    /// Empty filter.
    pub fn empty() -> Self {
        Self {
            mandatory: 0,
            optional: 0,
        }
    }

    /// Insert a mandatory item.
    pub fn add_mandatory<I: AsRef<[u8]>>(&mut self, item: I) {
        let (a, b) = bit_indices(item.as_ref());
        self.mandatory |= (1u128 << a) | (1u128 << b);
    }

    /// Insert an optional item.
    pub fn add_optional<I: AsRef<[u8]>>(&mut self, item: I) {
        let (a, b) = bit_indices(item.as_ref());
        self.optional |= (1u128 << a) | (1u128 << b);
    }

    /// Union of mandatory + optional bits (every item the peer claims).
    pub fn all(&self) -> u128 {
        self.mandatory | self.optional
    }

    /// True if this filter might contain `item` in either set.
    pub fn may_contain<I: AsRef<[u8]>>(&self, item: I) -> bool {
        let (a, b) = bit_indices(item.as_ref());
        let mask = (1u128 << a) | (1u128 << b);
        (self.all() & mask) == mask
    }

    /// True if this peer's full set of items covers every bit in
    /// `mine.mandatory`.
    pub fn covers_mandatory(&self, mine: &Self) -> bool {
        (self.all() & mine.mandatory) == mine.mandatory
    }

    /// True if this peer's full set of items shares at least one bit
    /// with `mine.optional`. Trivially false if `mine.optional` is empty.
    pub fn intersects_optional(&self, mine: &Self) -> bool {
        (self.all() & mine.optional) != 0
    }

    /// True if neither mandatory nor optional has any bit set.
    pub fn is_empty(&self) -> bool {
        self.mandatory == 0 && self.optional == 0
    }
}

/// Pick two bit positions in 0..128 from siphash-1-3(item). The two
/// halves of the 64-bit output give the two bloom hashes.
/// Fixed zero keys: the bloom is a public, deterministic identifier
/// every peer must compute the same way - this is not a hash table
/// where seed randomization helps.
fn bit_indices(bytes: &[u8]) -> (u32, u32) {
    let mut hasher = SipHasher13::new_with_keys(0, 0);
    hasher.write(bytes);
    let h = hasher.finish();
    let a = (h as u32) % 128;
    let b = ((h >> 32) as u32) % 128;
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_filter_contains_nothing() {
        let b = Bloom::empty();
        assert!(b.is_empty());
        assert!(!b.may_contain("X"));
    }

    #[test]
    fn add_mandatory_then_contains() {
        let mut b = Bloom::empty();
        b.add_mandatory("ChatProto");
        assert!(b.may_contain("ChatProto"));
    }

    #[test]
    fn add_optional_then_contains() {
        let mut b = Bloom::empty();
        b.add_optional("ChatProto");
        assert!(b.may_contain("ChatProto"));
    }

    #[test]
    fn covers_mandatory_subset() {
        let mut peer = Bloom::empty();
        peer.add_optional("X");
        peer.add_optional("Y");

        let mut mine = Bloom::empty();
        mine.add_mandatory("X");
        assert!(peer.covers_mandatory(&mine));

        mine.add_mandatory("Z");
        assert!(!peer.covers_mandatory(&mine));
    }

    #[test]
    fn intersects_optional_when_overlap() {
        let mut peer = Bloom::empty();
        peer.add_optional("Y");

        let mut mine = Bloom::empty();
        mine.add_optional("Y");
        assert!(peer.intersects_optional(&mine));

        let mut other = Bloom::empty();
        other.add_optional("Q");
        assert!(!peer.intersects_optional(&other));
    }

    #[test]
    fn covers_mandatory_uses_union_of_peer_sets() {
        let mut peer = Bloom::empty();
        peer.add_mandatory("X");
        peer.add_optional("Y");

        let mut mine = Bloom::empty();
        mine.add_mandatory("X");
        mine.add_mandatory("Y");
        assert!(peer.covers_mandatory(&mine));
    }

    #[test]
    fn empty_mandatory_is_always_covered() {
        let peer = Bloom::empty();
        let mine = Bloom::empty();
        assert!(peer.covers_mandatory(&mine));
    }

    #[test]
    fn deterministic_across_instances() {
        let mut a = Bloom::empty();
        a.add_mandatory("SomeProto");
        let mut b = Bloom::empty();
        b.add_mandatory("SomeProto");
        assert_eq!(a, b);
    }
}
