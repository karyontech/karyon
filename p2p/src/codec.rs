use std::time::Duration;

use bincode::{Decode, Encode};

use karyon_core::{
    async_util::timeout,
    util::{decode, encode, encode_into_slice},
};

use karyon_net::{Connection, NetError};

use crate::{
    message::{NetMsg, NetMsgCmd, NetMsgHeader, MAX_ALLOWED_MSG_SIZE, MSG_HEADER_SIZE},
    Error, Result,
};

pub trait CodecMsg: Decode + Encode + std::fmt::Debug {}
impl<T: Encode + Decode + std::fmt::Debug> CodecMsg for T {}

/// A Codec working with generic network connections.
///
/// It is responsible for both decoding data received from the network and
/// encoding data before sending it.
pub struct Codec {
    conn: Box<dyn Connection>,
}

impl Codec {
    /// Creates a new Codec.
    pub fn new(conn: Box<dyn Connection>) -> Self {
        Self { conn }
    }

    /// Reads a message of type `NetMsg` from the connection.
    ///
    /// It reads the first 6 bytes as the header of the message, then reads
    /// and decodes the remaining message data based on the determined header.
    pub async fn read(&self) -> Result<NetMsg> {
        // Read 6 bytes to get the header of the incoming message
        let mut buf = [0; MSG_HEADER_SIZE];
        self.read_exact(&mut buf).await?;

        // Decode the header from bytes to NetMsgHeader
        let (header, _) = decode::<NetMsgHeader>(&buf)?;

        if header.payload_size > MAX_ALLOWED_MSG_SIZE {
            return Err(Error::InvalidMsg(
                "Message exceeds the maximum allowed size".to_string(),
            ));
        }

        // Create a buffer to hold the message based on its length
        let mut payload = vec![0; header.payload_size as usize];
        self.read_exact(&mut payload).await?;

        Ok(NetMsg { header, payload })
    }

    /// Writes a message of type `T` to the connection.
    ///
    /// Before appending the actual message payload, it calculates the length of
    /// the encoded message in bytes and appends this length to the message header.
    pub async fn write<T: CodecMsg>(&self, command: NetMsgCmd, msg: &T) -> Result<()> {
        let payload = encode(msg)?;

        // Create a buffer to hold the message header (6 bytes)
        let header_buf = &mut [0; MSG_HEADER_SIZE];
        let header = NetMsgHeader {
            command,
            payload_size: payload.len() as u32,
        };
        encode_into_slice(&header, header_buf)?;

        let mut buffer = vec![];
        // Append the header bytes to the buffer
        buffer.extend_from_slice(header_buf);
        // Append the message payload to the buffer
        buffer.extend_from_slice(&payload);

        self.write_all(&buffer).await?;
        Ok(())
    }

    /// Reads a message of type `NetMsg` with the given timeout.
    pub async fn read_timeout(&self, duration: Duration) -> Result<NetMsg> {
        timeout(duration, self.read())
            .await
            .map_err(|_| NetError::Timeout)?
    }

    /// Reads the exact number of bytes required to fill `buf`.
    async fn read_exact(&self, mut buf: &mut [u8]) -> Result<()> {
        while !buf.is_empty() {
            let n = self.conn.read(buf).await?;
            let (_, rest) = std::mem::take(&mut buf).split_at_mut(n);
            buf = rest;

            if n == 0 {
                return Err(Error::IO(std::io::ErrorKind::UnexpectedEof.into()));
            }
        }

        Ok(())
    }

    /// Writes an entire buffer into the connection.
    async fn write_all(&self, mut buf: &[u8]) -> Result<()> {
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
}
