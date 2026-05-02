use bincode::{config, Decode, Encode};

use crate::Result;

fn bincode_config() -> impl config::Config {
    config::standard().with_fixed_int_encoding()
}

/// Encode the given type `T` into a `Vec<u8>`.
pub fn encode<T: Encode>(src: &T) -> Result<Vec<u8>> {
    Ok(bincode::encode_to_vec(src, bincode_config())?)
}

/// Decode a given type `T` from the given slice. Returns the decoded value
/// along with the number of bytes read.
pub fn decode<T: Decode<()>>(src: &[u8]) -> Result<(T, usize)> {
    let (result, bytes_read) = bincode::decode_from_slice::<T, _>(src, bincode_config())?;
    Ok((result, bytes_read))
}
