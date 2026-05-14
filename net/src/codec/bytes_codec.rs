use crate::{codec::Codec, ByteBuffer, Error, Result};

const MAX_BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4MB

#[derive(Clone)]
pub struct BytesCodec {
    max_size: usize,
}

impl Default for BytesCodec {
    fn default() -> Self {
        Self {
            max_size: MAX_BUFFER_SIZE,
        }
    }
}

impl BytesCodec {
    pub fn new(max_size: usize) -> Self {
        Self { max_size }
    }
}

impl Codec<ByteBuffer> for BytesCodec {
    type Message = Vec<u8>;
    type Error = Error;

    fn encode(&self, src: &Vec<u8>, dst: &mut ByteBuffer) -> Result<usize> {
        if src.len() > self.max_size {
            return Err(Error::BufferFull(format!(
                "Buffer size {} exceeds maximum {}",
                src.len(),
                self.max_size
            )));
        }

        dst.extend_from_slice(src);
        Ok(src.len())
    }

    fn decode(&self, src: &mut ByteBuffer) -> Result<Option<(usize, Vec<u8>)>> {
        if src.len() > self.max_size {
            return Err(Error::BufferFull(format!(
                "Buffer size {} exceeds maximum {}",
                src.len(),
                self.max_size
            )));
        }

        if src.is_empty() {
            Ok(None)
        } else {
            Ok(Some((src.len(), src.as_ref().to_vec())))
        }
    }
}
