use bincode::config;

use crate::{codec::Codec, ByteBuffer, Error, Result};

const MAX_BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4MB

/// The size of the message length.
const MSG_LENGTH_SIZE: usize = std::mem::size_of::<u32>();

#[derive(Clone)]
pub struct LengthCodec {
    max_size: usize,
}

impl LengthCodec {
    pub fn new(max_size: usize) -> Self {
        Self { max_size }
    }
}

impl Default for LengthCodec {
    fn default() -> Self {
        Self {
            max_size: MAX_BUFFER_SIZE,
        }
    }
}

fn bincode_config() -> impl config::Config {
    config::standard().with_fixed_int_encoding()
}

impl Codec<ByteBuffer> for LengthCodec {
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

        let length_buf = &mut [0u8; MSG_LENGTH_SIZE];
        bincode::encode_into_slice(src.len() as u32, length_buf, bincode_config())?;
        dst.extend_from_slice(length_buf);
        dst.extend_from_slice(src);

        Ok(dst.len())
    }

    fn decode(&self, src: &mut ByteBuffer) -> Result<Option<(usize, Vec<u8>)>> {
        if src.len() < MSG_LENGTH_SIZE {
            return Ok(None);
        }

        let mut length = [0u8; MSG_LENGTH_SIZE];
        length.copy_from_slice(&src.as_ref()[..MSG_LENGTH_SIZE]);
        let (length, _) = bincode::decode_from_slice::<u32, _>(&length, bincode_config())?;
        let length = length as usize;

        if length > self.max_size {
            return Err(Error::BufferFull(format!(
                "Frame length {} exceeds maximum {}",
                length, self.max_size
            )));
        }

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
