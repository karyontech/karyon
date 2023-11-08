use bincode::Encode;

use crate::Result;

/// Encode the given type `T` into a `Vec<u8>`.
pub fn encode<T: Encode>(msg: &T) -> Result<Vec<u8>> {
    let vec = bincode::encode_to_vec(msg, bincode::config::standard())?;
    Ok(vec)
}

/// Encode the given type `T` into the given slice..
pub fn encode_into_slice<T: Encode>(msg: &T, dst: &mut [u8]) -> Result<()> {
    bincode::encode_into_slice(msg, dst, bincode::config::standard())?;
    Ok(())
}
