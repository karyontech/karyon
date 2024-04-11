use crate::{
    codec::{Codec, Decoder, Encoder},
    Result,
};

#[derive(Clone)]
pub struct BytesCodec {}
impl Codec for BytesCodec {
    type Item = Vec<u8>;
}

impl Encoder for BytesCodec {
    type EnItem = Vec<u8>;
    fn encode(&self, src: &Self::EnItem, dst: &mut [u8]) -> Result<usize> {
        dst[..src.len()].copy_from_slice(src);
        Ok(src.len())
    }
}

impl Decoder for BytesCodec {
    type DeItem = Vec<u8>;
    fn decode(&self, src: &mut [u8]) -> Result<Option<(usize, Self::DeItem)>> {
        if src.is_empty() {
            Ok(None)
        } else {
            Ok(Some((src.len(), src.to_vec())))
        }
    }
}
