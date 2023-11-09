use super::{Entry, Key};

use rand::{rngs::OsRng, seq::SliceRandom};

/// BITFLAGS represent the status of an Entry within a bucket.
pub type EntryStatusFlag = u16;

/// The entry is connected.
pub const CONNECTED_ENTRY: EntryStatusFlag = 0b00001;

/// The entry is disconnected. This will increase the failure counter.
pub const DISCONNECTED_ENTRY: EntryStatusFlag = 0b00010;

/// The entry is ready to reconnect, meaning it has either been added and
/// has no connection attempts, or it has been refreshed.
pub const PENDING_ENTRY: EntryStatusFlag = 0b00100;

/// The entry is unreachable. This will increase the failure counter.
pub const UNREACHABLE_ENTRY: EntryStatusFlag = 0b01000;

/// The entry is unstable. This will increase the failure counter.
pub const UNSTABLE_ENTRY: EntryStatusFlag = 0b10000;

#[allow(dead_code)]
pub const ALL_ENTRY: EntryStatusFlag = 0b11111;

/// A BucketEntry represents a peer in the routing table.
#[derive(Clone, Debug)]
pub struct BucketEntry {
    pub status: EntryStatusFlag,
    pub entry: Entry,
    pub failures: u32,
    pub last_seen: i64,
}

impl BucketEntry {
    pub fn is_connected(&self) -> bool {
        self.status ^ CONNECTED_ENTRY == 0
    }

    pub fn is_unreachable(&self) -> bool {
        self.status ^ UNREACHABLE_ENTRY == 0
    }

    pub fn is_unstable(&self) -> bool {
        self.status ^ UNSTABLE_ENTRY == 0
    }
}

/// The number of entries that can be stored within a single bucket.
pub const BUCKET_SIZE: usize = 20;

/// A Bucket represents a group of entries in the routing table.
#[derive(Debug, Clone)]
pub struct Bucket {
    entries: Vec<BucketEntry>,
}

impl Bucket {
    /// Creates a new empty Bucket
    pub fn new() -> Self {
        Self {
            entries: Vec::with_capacity(BUCKET_SIZE),
        }
    }

    /// Add an entry to the bucket.
    pub fn add(&mut self, entry: &Entry) {
        self.entries.push(BucketEntry {
            status: PENDING_ENTRY,
            entry: entry.clone(),
            failures: 0,
            last_seen: chrono::Utc::now().timestamp(),
        })
    }

    /// Get the number of entries in the bucket.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns an iterator over the entries in the bucket.
    pub fn iter(&self) -> impl Iterator<Item = &BucketEntry> {
        self.entries.iter()
    }

    /// Remove an entry.
    pub fn remove(&mut self, key: &Key) {
        let position = self.entries.iter().position(|e| &e.entry.key == key);
        if let Some(i) = position {
            self.entries.remove(i);
        }
    }

    /// Returns an iterator of entries in random order.
    pub fn random_iter(&self, amount: usize) -> impl Iterator<Item = &BucketEntry> {
        self.entries.choose_multiple(&mut OsRng, amount)
    }

    /// Updates the status of an entry in the bucket identified by the given key.
    ///
    /// If the key is not found in the bucket, no action is taken.
    ///
    /// This will also update the last_seen field and increase the failures
    /// counter for the bucket entry according to the new status.
    pub fn update_entry(&mut self, key: &Key, entry_flag: EntryStatusFlag) {
        if let Some(e) = self.entries.iter_mut().find(|e| &e.entry.key == key) {
            e.status = entry_flag;
            if e.is_unreachable() || e.is_unstable() {
                e.failures += 1;
            }

            if !e.is_unreachable() {
                e.last_seen = chrono::Utc::now().timestamp();
            }
        }
    }

    /// Check if the bucket contains the given key.
    pub fn contains_key(&self, key: &Key) -> bool {
        self.entries.iter().any(|e| &e.entry.key == key)
    }
}
