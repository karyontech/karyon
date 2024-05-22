mod decode;
mod encode;
mod path;

pub use decode::decode;
pub use encode::{encode, encode_into_slice};
pub use path::{home_dir, tilde_expand};

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
