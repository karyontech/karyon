use rand::{rngs::OsRng, Rng};

/// Generates and returns a random u32 using `rand::rngs::OsRng`.
pub fn random_32() -> u32 {
    OsRng.gen()
}

/// Generates and returns a random u64 using `rand::rngs::OsRng`.
pub fn random_64() -> u64 {
    OsRng.gen()
}

/// Generates and returns a random u16 using `rand::rngs::OsRng`.
pub fn random_16() -> u16 {
    OsRng.gen()
}
