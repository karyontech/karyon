use bincode::Decode;

use crate::Result;

/// Decodes a given type `T` from the given slice. returns the decoded value
/// along with the number of bytes read.
pub fn decode<T: Decode<()>>(src: &[u8]) -> Result<(T, usize)> {
    let (result, bytes_read) = bincode::decode_from_slice::<T, _>(
        src,
        bincode::config::standard().with_fixed_int_encoding(),
    )?;
    Ok((result, bytes_read))
}
