use memchr::memchr;

use karyons_net::Conn;

use crate::{Error, Result};

const DEFAULT_MSG_SIZE: usize = 1024;
const MAX_ALLOWED_MSG_SIZE: usize = 1024 * 1024; // 1MB

// TODO: Add unit tests for these functions.

/// Read all bytes into `buffer` until the `0x0` byte or EOF is
/// reached.
///
/// If successful, this function will return the total number of bytes read.
pub async fn read_until(conn: &Conn, buffer: &mut Vec<u8>) -> Result<usize> {
    let delim = b'\0';

    let mut read = 0;

    loop {
        let mut tmp_buf = [0; DEFAULT_MSG_SIZE];
        let n = conn.read(&mut tmp_buf).await?;
        if n == 0 {
            return Err(Error::IO(std::io::ErrorKind::UnexpectedEof.into()));
        }

        match memchr(delim, &tmp_buf) {
            Some(i) => {
                buffer.extend_from_slice(&tmp_buf[..i]);
                read += i;
                break;
            }
            None => {
                buffer.extend_from_slice(&tmp_buf);
                read += tmp_buf.len();
            }
        }

        if buffer.len() == MAX_ALLOWED_MSG_SIZE {
            return Err(Error::InvalidMsg(
                "Message exceeds the maximum allowed size",
            ));
        }
    }

    Ok(read)
}

/// Writes an entire buffer into the given connection.
pub async fn write_all(conn: &Conn, mut buf: &[u8]) -> Result<()> {
    while !buf.is_empty() {
        let n = conn.write(buf).await?;
        let (_, rest) = std::mem::take(&mut buf).split_at(n);
        buf = rest;

        if n == 0 {
            return Err(Error::IO(std::io::ErrorKind::UnexpectedEof.into()));
        }
    }

    Ok(())
}
