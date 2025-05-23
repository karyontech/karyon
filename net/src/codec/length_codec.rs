use karyon_core::util::{decode, encode_into_slice};

use crate::{
    codec::{ByteBuffer, Codec, Decoder, Encoder},
    Error, Result,
};

/// The size of the message length.
const MSG_LENGTH_SIZE: usize = std::mem::size_of::<u32>();

#[derive(Clone)]
pub struct LengthCodec {}
impl Codec for LengthCodec {
    type Message = Vec<u8>;
    type Error = Error;
}

impl Encoder for LengthCodec {
    type EnMessage = Vec<u8>;
    type EnError = Error;
    fn encode(&self, src: &Self::EnMessage, dst: &mut ByteBuffer) -> Result<usize> {
        let length_buf = &mut [0; MSG_LENGTH_SIZE];
        encode_into_slice(&(src.len() as u32), length_buf)?;

        dst.resize(MSG_LENGTH_SIZE);
        dst.extend_from_slice(length_buf);

        dst.resize(src.len());
        dst.extend_from_slice(src);

        Ok(dst.len())
    }
}

impl Decoder for LengthCodec {
    type DeMessage = Vec<u8>;
    type DeError = Error;
    fn decode(&self, src: &mut ByteBuffer) -> Result<Option<(usize, Self::DeMessage)>> {
        if src.len() < MSG_LENGTH_SIZE {
            return Ok(None);
        }

        let mut length = [0; MSG_LENGTH_SIZE];
        length.copy_from_slice(&src.as_ref()[..MSG_LENGTH_SIZE]);
        let (length, _) = decode::<u32>(&length)?;
        let length = length as usize;

        if src.len() - MSG_LENGTH_SIZE >= length {
            Ok(Some((
                length + MSG_LENGTH_SIZE,
                src.as_ref()[MSG_LENGTH_SIZE..length + MSG_LENGTH_SIZE].to_vec(),
            )))
        } else {
            Ok(None)
        }
    }
}
