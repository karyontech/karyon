use crate::{
    codec::{ByteBuffer, Codec, Decoder, Encoder},
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
    fn encode(&self, src: &Self::EnMessage, dst: &mut ByteBuffer) -> Result<usize> {
        dst.extend_from_slice(src);
        Ok(src.len())
    }
}

impl Decoder for BytesCodec {
    type DeMessage = Vec<u8>;
    type DeError = Error;
    fn decode(&self, src: &mut ByteBuffer) -> Result<Option<(usize, Self::DeMessage)>> {
        if src.is_empty() {
            Ok(None)
        } else {
            Ok(Some((src.len(), src.as_ref().to_vec())))
        }
    }
}
