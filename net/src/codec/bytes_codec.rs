use crate::{
    codec::{Codec, Decoder, Encoder},
    Error, Result,
};

#[derive(Clone)]
pub struct BytesCodec {}
impl Codec for BytesCodec {
    type Message = Vec<u8>;
    type Error = Error;
}

impl Encoder for BytesCodec {
    type EnMessage = Vec<u8>;
    type EnError = Error;
    fn encode(&self, src: &Self::EnMessage, dst: &mut [u8]) -> Result<usize> {
        dst[..src.len()].copy_from_slice(src);
        Ok(src.len())
    }
}

impl Decoder for BytesCodec {
    type DeMessage = Vec<u8>;
    type DeError = Error;
    fn decode(&self, src: &mut [u8]) -> Result<Option<(usize, Self::DeMessage)>> {
        if src.is_empty() {
            Ok(None)
        } else {
            Ok(Some((src.len(), src.to_vec())))
        }
    }
}
