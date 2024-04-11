use bincode::Encode;

use crate::{Error, Result};

/// Encode the given type `T` into a `Vec<u8>`.
pub fn encode<T: Encode>(src: &T) -> Result<Vec<u8>> {
    let vec = bincode::encode_to_vec(src, bincode::config::standard())?;
    Ok(vec)
}

/// Encode the given type `T` into the given slice..
pub fn encode_into_slice<T: Encode>(src: &T, dst: &mut [u8]) -> Result<usize> {
    bincode::encode_into_slice(src, dst, bincode::config::standard()).map_err(Error::from)
}
