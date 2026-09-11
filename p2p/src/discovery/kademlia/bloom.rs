use std::{hash::Hasher, sync::Arc};

use bincode::{Decode, Encode};
use parking_lot::Mutex;
use siphasher::sip::SipHasher13;

use crate::protocol::ProtocolFlags;

/// 128-bit bloom filter (k=2 hashes) of items a peer supports.
///
/// Content-agnostic: items can be protocol ids, swarm keys, or any
/// other identifier hashable as bytes. Discovery-layer hint, not an
/// authoritative list.
#[derive(Encode, Decode, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bloom(u128);

impl Bloom {
    /// Insert an item.
    pub fn add<I: AsRef<[u8]>>(&mut self, item: I) {
        self.0 |= item_mask(item.as_ref());
    }

    /// True if this filter might contain `item`.
    pub fn may_contain<I: AsRef<[u8]>>(&self, item: I) -> bool {
        let mask = item_mask(item.as_ref());
        (self.0 & mask) == mask
    }

    /// True if every bit set in `other` is also set in `self`.
    /// An empty `other` is always covered.
    pub fn covers(&self, other: &Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// True if `self` and `other` share at least one bit.
    pub fn intersects(&self, other: &Self) -> bool {
        (self.0 & other.0) != 0
    }

    /// True if no bit is set.
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }
}

/// What the local node advertises and how it filters peers.
///
/// `advertised` is sent on the wire. `required` and `preferred` never
/// leave the node; they drive routing-table filtering: a peer must
/// cover `required` and, when `preferred` is non-empty, intersect it.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalBloom {
    pub advertised: Bloom,
    pub required: Bloom,
    pub preferred: Bloom,
}

impl LocalBloom {
    /// True if `peer`'s advertised bloom is acceptable to this node.
    pub fn matches(&self, peer: &Bloom) -> bool {
        if !peer.covers(&self.required) {
            return false;
        }
        if self.preferred.is_empty() {
            return true;
        }
        peer.intersects(&self.preferred)
    }
}

/// Shared handle to the local bloom state. Writers add items in
/// place, readers take a `snapshot`. Cheap to clone (Arc).
#[derive(Clone, Debug, Default)]
pub struct BloomRef {
    inner: Arc<Mutex<LocalBloom>>,
}

impl BloomRef {
    /// Empty shared bloom.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an item. Any non-empty flags advertise it; `REQUIRED` and
    /// `PREFERRED` also mark it for local filtering. Other bits only
    /// advertise.
    pub fn add<I: AsRef<[u8]>>(&self, item: I, flags: ProtocolFlags) {
        if flags == ProtocolFlags::empty() {
            return;
        }
        let item = item.as_ref();
        let mut local = self.inner.lock();
        local.advertised.add(item);
        if flags.contains(ProtocolFlags::REQUIRED) {
            local.required.add(item);
        }
        if flags.contains(ProtocolFlags::PREFERRED) {
            local.preferred.add(item);
        }
    }

    /// Copy of the current local bloom state.
    pub fn snapshot(&self) -> LocalBloom {
        *self.inner.lock()
    }
}

/// Bit mask for `item`: two bit positions in 0..128 from
/// siphash-1-3(item). The two halves of the 64-bit output give the
/// two bloom hashes. Fixed zero keys: the bloom is a public,
/// deterministic identifier every peer must compute the same way.
fn item_mask(bytes: &[u8]) -> u128 {
    let mut hasher = SipHasher13::new_with_keys(0, 0);
    hasher.write(bytes);
    let h = hasher.finish();
    let a = (h as u32) % 128;
    let b = ((h >> 32) as u32) % 128;
    (1u128 << a) | (1u128 << b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_filter_contains_nothing() {
        let b = Bloom::default();
        assert!(b.is_empty());
        assert!(!b.may_contain("X"));
    }

    #[test]
    fn add_then_contains() {
        let mut b = Bloom::default();
        b.add("ChatProto");
        assert!(b.may_contain("ChatProto"));
    }

    #[test]
    fn covers_subset() {
        let mut peer = Bloom::default();
        peer.add("X");
        peer.add("Y");

        let mut mine = Bloom::default();
        mine.add("X");
        assert!(peer.covers(&mine));

        mine.add("Z");
        assert!(!peer.covers(&mine));
    }

    #[test]
    fn empty_is_always_covered() {
        assert!(Bloom::default().covers(&Bloom::default()));
    }

    #[test]
    fn intersects_when_overlap() {
        let mut peer = Bloom::default();
        peer.add("Y");

        let mut mine = Bloom::default();
        mine.add("Y");
        assert!(peer.intersects(&mine));

        let mut other = Bloom::default();
        other.add("Q");
        assert!(!peer.intersects(&other));
    }

    #[test]
    fn deterministic_across_instances() {
        let mut a = Bloom::default();
        a.add("SomeProto");
        let mut b = Bloom::default();
        b.add("SomeProto");
        assert_eq!(a, b);
    }

    #[test]
    fn local_matches_required_and_preferred() {
        let bloom = BloomRef::new();
        bloom.add("Ping", ProtocolFlags::REQUIRED);
        bloom.add("Chat", ProtocolFlags::PREFERRED);
        let local = bloom.snapshot();

        let mut peer = Bloom::default();
        assert!(!local.matches(&peer));
        peer.add("Ping");
        assert!(!local.matches(&peer));
        peer.add("Chat");
        assert!(local.matches(&peer));
    }

    #[test]
    fn empty_local_matches_everything() {
        let local = LocalBloom::default();
        assert!(local.matches(&Bloom::default()));
    }

    #[test]
    fn custom_flags_only_advertise() {
        let bloom = BloomRef::new();
        bloom.add("Room", ProtocolFlags::USER);
        let local = bloom.snapshot();
        assert!(local.advertised.may_contain("Room"));
        assert!(local.required.is_empty());
        assert!(local.preferred.is_empty());
    }

    #[test]
    fn empty_flags_do_nothing() {
        let bloom = BloomRef::new();
        bloom.add("Hidden", ProtocolFlags::empty());
        assert!(bloom.snapshot().advertised.is_empty());
    }
}
