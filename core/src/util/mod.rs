use rand::Rng;

/// Generates and returns a random u32.
pub fn random_32() -> u32 {
    rand::rng().random()
}

/// Generates and returns a random u64.
pub fn random_64() -> u64 {
    rand::rng().random()
}

/// Generates and returns a random u16.
pub fn random_16() -> u16 {
    rand::rng().random()
}
