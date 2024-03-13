use memchr::memchr;

use karyon_core::async_util::timeout;
use karyon_net::Conn;

use crate::{Error, Result};

const DEFAULT_BUFFER_SIZE: usize = 1024;
const DEFAULT_MAX_ALLOWED_BUFFER_SIZE: usize = 1024 * 1024; // 1MB

// TODO: Add unit tests for Codec's functions.

/// Represents Codec config
#[derive(Clone)]
pub struct CodecConfig {
    pub default_buffer_size: usize,
    /// The maximum allowed buffer size to receive a message. If set to zero,
    /// there will be no size limit.
    pub max_allowed_buffer_size: usize,
}

impl Default for CodecConfig {
    fn default() -> Self {
        Self {
            default_buffer_size: DEFAULT_BUFFER_SIZE,
            max_allowed_buffer_size: DEFAULT_MAX_ALLOWED_BUFFER_SIZE,
        }
    }
}

pub struct Codec {
    conn: Conn,
    config: CodecConfig,
}

impl Codec {
    /// Creates a new Codec
    pub fn new(conn: Conn, config: CodecConfig) -> Self {
        Self { conn, config }
    }

    /// Read all bytes into `buffer` until the `0x0A` byte or EOF is
    /// reached.
    ///
    /// If successful, this function will return the total number of bytes read.
    pub async fn read_until(&self, buffer: &mut Vec<u8>) -> Result<usize> {
        let delim = b'\n';

        let mut read = 0;

        loop {
            let mut tmp_buf = vec![0; self.config.default_buffer_size];
            let n = self.conn.read(&mut tmp_buf).await?;
            if n == 0 {
                return Err(Error::IO(std::io::ErrorKind::UnexpectedEof.into()));
            }

            match memchr(delim, &tmp_buf) {
                Some(i) => {
                    buffer.extend_from_slice(&tmp_buf[..=i]);
                    read += i + 1;
                    break;
                }
                None => {
                    buffer.extend_from_slice(&tmp_buf);
                    read += tmp_buf.len();
                }
            }

            if self.config.max_allowed_buffer_size != 0
                && buffer.len() == self.config.max_allowed_buffer_size
            {
                return Err(Error::InvalidMsg(
                    "Message exceeds the maximum allowed size",
                ));
            }
        }

        Ok(read)
    }

    /// Writes an entire buffer into the given connection.
    pub async fn write_all(&self, mut buf: &[u8]) -> Result<()> {
        while !buf.is_empty() {
            let n = self.conn.write(buf).await?;
            let (_, rest) = std::mem::take(&mut buf).split_at(n);
            buf = rest;

            if n == 0 {
                return Err(Error::IO(std::io::ErrorKind::UnexpectedEof.into()));
            }
        }

        Ok(())
    }

    pub async fn read_until_with_timeout(&self, buffer: &mut Vec<u8>, t: u64) -> Result<usize> {
        timeout(std::time::Duration::from_secs(t), self.read_until(buffer)).await?
    }
}
