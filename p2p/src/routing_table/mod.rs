mod bucket;
mod entry;

use std::net::IpAddr;

use parking_lot::RwLock;

use rand::{rngs::OsRng, seq::SliceRandom};

use karyon_net::Addr;

pub use bucket::{
    Bucket, BucketEntry, EntryStatusFlag, CONNECTED_ENTRY, DISCONNECTED_ENTRY, INCOMPATIBLE_ENTRY,
    PENDING_ENTRY, UNREACHABLE_ENTRY, UNSTABLE_ENTRY,
};
pub use entry::{xor_distance, Entry, Key};

use bucket::BUCKET_SIZE;
use entry::KEY_SIZE;

/// The total number of buckets in the routing table.
const TABLE_SIZE: usize = 32;

/// The distance limit for the closest buckets.
const DISTANCE_LIMIT: usize = 32;

/// The maximum number of matched subnets allowed within a single bucket.
const MAX_MATCHED_SUBNET_IN_BUCKET: usize = 1;

/// The maximum number of matched subnets across the entire routing table.
const MAX_MATCHED_SUBNET_IN_TABLE: usize = 6;

/// Represents the possible result when adding a new entry.
#[derive(Debug)]
pub enum AddEntryResult {
    /// The entry is added.
    Added,
    /// The entry is already exists.
    Exists,
    /// The entry is ignored.
    Ignored,
    /// The entry is restricted and not allowed.
    Restricted,
}

/// This is a modified version of the Kademlia Distributed Hash Table (DHT).
/// <https://en.wikipedia.org/wiki/Kademlia>
#[derive(Debug)]
pub struct RoutingTable {
    key: Key,
    buckets: RwLock<Vec<Bucket>>,
}

impl RoutingTable {
    /// Creates a new RoutingTable
    pub fn new(key: Key) -> Self {
        let buckets: Vec<Bucket> = (0..TABLE_SIZE).map(|_| Bucket::new()).collect();
        Self {
            key,
            buckets: RwLock::new(buckets),
        }
    }

    /// Adds a new entry to the table and returns a result indicating success,
    /// failure, or restrictions.
    pub fn add_entry(&self, entry: Entry) -> AddEntryResult {
        // Determine the index of the bucket where the entry should be placed.
        let bucket_idx = match self.bucket_index(&entry.key) {
            Some(i) => i,
            None => return AddEntryResult::Ignored,
        };

        // Check if the entry already exists in the bucket.
        if self.contains_key(&entry.key) {
            return AddEntryResult::Exists;
        }

        // Check if the entry is restricted.
        if self.subnet_restricted(bucket_idx, &entry) {
            return AddEntryResult::Restricted;
        }

        let mut buckets = self.buckets.write();
        let bucket = &mut buckets[bucket_idx];

        // If the bucket has free space, add the entry and return success.
        if bucket.len() < BUCKET_SIZE {
            bucket.add(&entry);
            return AddEntryResult::Added;
        }

        // Replace it with an incompatible entry if one exists.
        let incompatible_entry = bucket.iter().find(|e| e.is_incompatible()).cloned();
        if let Some(e) = incompatible_entry {
            bucket.remove(&e.entry.key);
            bucket.add(&entry);
            return AddEntryResult::Added;
        }

        // If the bucket is full, the entry is ignored.
        AddEntryResult::Ignored
    }

    /// Check if the table contains the given key.
    pub fn contains_key(&self, key: &Key) -> bool {
        let buckets = self.buckets.read();
        // Determine the bucket index for the given key.
        let bucket_idx = match self.bucket_index(key) {
            Some(bi) => bi,
            None => return false,
        };

        let bucket = &buckets[bucket_idx];
        bucket.contains_key(key)
    }

    /// Updates the status of an entry in the routing table identified
    /// by the given key.
    ///
    /// If the key is not found, no action is taken.
    pub fn update_entry(&self, key: &Key, entry_flag: EntryStatusFlag) {
        let mut buckets = self.buckets.write();
        // Determine the bucket index for the given key.
        let bucket_idx = match self.bucket_index(key) {
            Some(bi) => bi,
            None => return,
        };

        let bucket = &mut buckets[bucket_idx];
        bucket.update_entry(key, entry_flag);
    }

    /// Returns a list of bucket indexes that are closest to the given target key.
    pub fn bucket_indexes(&self, target_key: &Key) -> Vec<usize> {
        let mut indexes = vec![];

        // Determine the primary bucket index for the target key.
        let bucket_idx = self.bucket_index(target_key).unwrap_or(0);

        indexes.push(bucket_idx);

        // Add additional bucket indexes within a certain distance limit.
        for i in 1..DISTANCE_LIMIT {
            if bucket_idx >= i && bucket_idx - i >= 1 {
                indexes.push(bucket_idx - i);
            }

            if bucket_idx + i < (TABLE_SIZE - 1) {
                indexes.push(bucket_idx + i);
            }
        }

        indexes
    }

    /// Returns a list of the closest entries to the given target key, limited by max_entries.
    pub fn closest_entries(&self, target_key: &Key, max_entries: usize) -> Vec<Entry> {
        let buckets = self.buckets.read();
        let mut entries: Vec<Entry> = vec![];

        // Collect entries
        'outer: for idx in self.bucket_indexes(target_key) {
            let bucket = &buckets[idx];
            for bucket_entry in bucket.iter() {
                if bucket_entry.is_unreachable() || bucket_entry.is_unstable() {
                    continue;
                }

                entries.push(bucket_entry.entry.clone());
                if entries.len() == max_entries {
                    break 'outer;
                }
            }
        }

        // Sort the entries by their distance to the target key.
        entries.sort_by(|a, b| {
            xor_distance(target_key, &a.key).cmp(&xor_distance(target_key, &b.key))
        });

        entries
    }

    /// Removes an entry with the given key from the routing table, if it exists.
    pub fn remove_entry(&self, key: &Key) {
        let mut buckets = self.buckets.write();
        // Determine the bucket index for the given key.
        let bucket_idx = match self.bucket_index(key) {
            Some(bi) => bi,
            None => return,
        };

        let bucket = &mut buckets[bucket_idx];
        bucket.remove(key);
    }

    /// Returns an iterator of entries.
    /// FIXME: TODO: avoid cloning the data
    pub fn buckets(&self) -> Vec<Bucket> {
        self.buckets.read().clone()
    }

    /// Returns a random entry from the routing table.
    pub fn random_entry(&self, entry_flag: EntryStatusFlag) -> Option<Entry> {
        let buckets = self.buckets.read();
        for bucket in buckets.choose_multiple(&mut OsRng, buckets.len()) {
            for entry in bucket.random_iter(bucket.len()) {
                if entry.status & entry_flag == 0 {
                    continue;
                }
                return Some(entry.entry.clone());
            }
        }

        None
    }

    // Returns the bucket index for a given key in the table.
    fn bucket_index(&self, key: &Key) -> Option<usize> {
        // Calculate the XOR distance between the self key and the provided key.
        let distance = xor_distance(&self.key, key);

        for (i, b) in distance.iter().enumerate() {
            if *b != 0 {
                let lz = i * 8 + b.leading_zeros() as usize;
                let bits = KEY_SIZE * 8 - 1;
                let idx = (bits - lz) / 8;
                return Some(idx);
            }
        }
        None
    }

    /// This function iterate through the routing table and counts how many
    /// entries in the same subnet as the given Entry are already present.
    ///
    /// If the number of matching entries in the same bucket exceeds a
    /// threshold (MAX_MATCHED_SUBNET_IN_BUCKET), or if the total count of
    /// matching entries in the entire table exceeds a threshold
    /// (MAX_MATCHED_SUBNET_IN_TABLE), the addition of the Entry
    /// is considered restricted and returns true.
    fn subnet_restricted(&self, idx: usize, entry: &Entry) -> bool {
        let buckets = self.buckets.read();
        let mut bucket_count = 0;
        let mut table_count = 0;

        // Iterate through the routing table's buckets and entries to check
        // for subnet matches.
        for (i, bucket) in buckets.iter().enumerate() {
            for e in bucket.iter() {
                // If there is a subnet match, update the counts.
                let matched = subnet_match(&e.entry.addr, &entry.addr);
                if matched {
                    if i == idx {
                        bucket_count += 1;
                    }
                    table_count += 1;
                }

                // If the number of matched entries in the same bucket exceeds
                // the limit, return true
                if bucket_count >= MAX_MATCHED_SUBNET_IN_BUCKET {
                    return true;
                }
            }

            // If the total matched entries in the table exceed the limit,
            // return true.
            if table_count >= MAX_MATCHED_SUBNET_IN_TABLE {
                return true;
            }
        }

        // If no subnet restrictions are encountered, return false.
        false
    }
}

/// Check if two addresses belong to the same subnet.
fn subnet_match(addr: &Addr, other_addr: &Addr) -> bool {
    match (addr, other_addr) {
        (Addr::Ip(IpAddr::V4(ip)), Addr::Ip(IpAddr::V4(other_ip))) => {
            // TODO: Consider moving this to a different place
            if other_ip.is_loopback() && ip.is_loopback() {
                return false;
            }
            ip.octets()[0..3] == other_ip.octets()[0..3]
        }
        // Assume that they have /64 prefix
        (Addr::Ip(IpAddr::V6(ip)), Addr::Ip(IpAddr::V6(other_ip))) => {
            if other_ip.is_loopback() && ip.is_loopback() {
                return false;
            }
            // Compare the first 4 segments (128 bits, 4 * 16 bits)
            ip.segments()[0..4] == other_ip.segments()[0..4]
        }

        // If the address types don't match or are not handled
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::bucket::ALL_ENTRY;
    use super::*;

    use karyon_net::Addr;

    struct Setup {
        local_key: Key,
        keys: Vec<Key>,
    }

    fn new_entry(key: &Key, addr: &Addr, port: u16, discovery_port: u16) -> Entry {
        Entry {
            key: *key,
            addr: addr.clone(),
            port,
            discovery_port,
        }
    }

    impl Setup {
        fn new() -> Self {
            let keys = vec![
                [
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 1,
                ],
                [
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    1, 1, 0, 1, 1, 2,
                ],
                [
                    0, 0, 0, 0, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 3,
                ],
                [
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 30, 1, 18, 0, 0, 0,
                    0, 0, 0, 0, 0, 4,
                ],
                [
                    223, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 5,
                ],
                [
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 1, 18, 0, 0, 0,
                    0, 0, 0, 0, 0, 6,
                ],
                [
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 50, 1, 18, 0, 0,
                    0, 0, 0, 0, 0, 0, 7,
                ],
                [
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 10, 50, 1, 18, 0, 0,
                    0, 0, 0, 0, 0, 0, 8,
                ],
                [
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 10, 10, 50, 1, 18, 0, 0,
                    0, 0, 0, 0, 0, 0, 9,
                ],
            ];

            Self {
                local_key: [
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0,
                ],
                keys,
            }
        }

        fn entries(&self) -> Vec<Entry> {
            let mut entries = vec![];
            for (i, key) in self.keys.iter().enumerate() {
                entries.push(new_entry(
                    key,
                    &Addr::Ip(format!("127.0.{i}.1").parse().unwrap()),
                    3000,
                    3010,
                ));
            }
            entries
        }

        fn table(&self) -> RoutingTable {
            let table = RoutingTable::new(self.local_key);

            for entry in self.entries() {
                let res = table.add_entry(entry);
                assert!(matches!(res, AddEntryResult::Added));
            }

            table
        }
    }

    #[test]
    fn test_bucket_index() {
        let setup = Setup::new();
        let table = setup.table();

        assert_eq!(table.bucket_index(&setup.local_key), None);
        assert_eq!(table.bucket_index(&setup.keys[0]), Some(0));
        assert_eq!(table.bucket_index(&setup.keys[1]), Some(5));
        assert_eq!(table.bucket_index(&setup.keys[2]), Some(26));
        assert_eq!(table.bucket_index(&setup.keys[3]), Some(11));
        assert_eq!(table.bucket_index(&setup.keys[4]), Some(31));
        assert_eq!(table.bucket_index(&setup.keys[5]), Some(11));
        assert_eq!(table.bucket_index(&setup.keys[6]), Some(12));
        assert_eq!(table.bucket_index(&setup.keys[7]), Some(13));
        assert_eq!(table.bucket_index(&setup.keys[8]), Some(14));
    }

    #[test]
    fn test_closest_entries() {
        let setup = Setup::new();
        let table = setup.table();
        let entries = setup.entries();

        assert_eq!(
            table.closest_entries(&setup.keys[5], 8),
            vec![
                entries[5].clone(),
                entries[3].clone(),
                entries[1].clone(),
                entries[6].clone(),
                entries[7].clone(),
                entries[8].clone(),
                entries[2].clone(),
            ]
        );

        assert_eq!(
            table.closest_entries(&setup.keys[4], 2),
            vec![entries[4].clone(), entries[2].clone()]
        );
    }

    #[test]
    fn test_random_entry() {
        let setup = Setup::new();
        let table = setup.table();
        let entries = setup.entries();

        let entry = table.random_entry(ALL_ENTRY);
        assert!(entry.is_some());

        let entry = table.random_entry(CONNECTED_ENTRY);
        assert!(entry.is_none());

        for entry in entries {
            table.remove_entry(&entry.key);
        }

        let entry = table.random_entry(ALL_ENTRY);
        assert!(entry.is_none());
    }

    #[test]
    fn test_add_entries() {
        let setup = Setup::new();
        let table = setup.table();

        let key = [
            0, 0, 0, 0, 0, 0, 0, 1, 3, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 5,
        ];

        let key2 = [
            0, 0, 0, 0, 0, 0, 0, 1, 2, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 5,
        ];

        let entry1 = new_entry(&key, &Addr::Ip("240.120.3.1".parse().unwrap()), 3000, 3010);
        assert!(matches!(
            table.add_entry(entry1.clone()),
            AddEntryResult::Added
        ));

        assert!(matches!(table.add_entry(entry1), AddEntryResult::Exists));

        let entry2 = new_entry(&key2, &Addr::Ip("240.120.3.2".parse().unwrap()), 3000, 3010);
        assert!(matches!(
            table.add_entry(entry2),
            AddEntryResult::Restricted
        ));

        let mut key: [u8; 32] = [0; 32];

        for i in 0..BUCKET_SIZE {
            key[i] += 1;
            let entry = new_entry(
                &key,
                &Addr::Ip(format!("127.0.{i}.1").parse().unwrap()),
                3000,
                3010,
            );
            table.add_entry(entry);
        }

        key[BUCKET_SIZE] += 1;
        let entry = new_entry(&key, &Addr::Ip("125.20.0.1".parse().unwrap()), 3000, 3010);
        assert!(matches!(table.add_entry(entry), AddEntryResult::Ignored));
    }

    use std::net::{Ipv4Addr, Ipv6Addr};
    #[test]
    fn check_subnet_match() {
        let addr_v4 = Addr::Ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));
        let other_addr_v4 = Addr::Ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)));

        let addr_v6 = Addr::Ip(IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)));
        let other_addr_v6 = Addr::Ip(IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 2)));
        let diff_addr_v6 = Addr::Ip(IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb7, 0, 0, 0, 0, 0, 2)));

        assert!(subnet_match(&addr_v4, &other_addr_v4));
        assert!(subnet_match(&addr_v6, &other_addr_v6));
        assert!(!subnet_match(&addr_v6, &diff_addr_v6));
    }
}
