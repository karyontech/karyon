use std::time::Duration;

use bincode::{Decode, Encode};

use karyons_core::{
    async_utils::timeout,
    utils::{decode, encode, encode_into_slice},
};

use karyons_net::{Connection, NetError};

use crate::{
    message::{NetMsg, NetMsgCmd, NetMsgHeader, MAX_ALLOWED_MSG_SIZE, MSG_HEADER_SIZE},
    Error, Result,
};

pub trait CodecMsg: Decode + Encode + std::fmt::Debug {}
impl<T: Encode + Decode + std::fmt::Debug> CodecMsg for T {}

/// I/O codec working with generic network connections.
///
/// It is responsible for both decoding data received from the network and
/// encoding data before sending it.
pub struct IOCodec {
    conn: Box<dyn Connection>,
}

impl IOCodec {
    /// Creates a new IOCodec.
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
        self.conn.recv(&mut buf).await?;

        // Decode the header from bytes to NetMsgHeader
        let (header, _) = decode::<NetMsgHeader>(&buf)?;

        if header.payload_size > MAX_ALLOWED_MSG_SIZE {
            return Err(Error::InvalidMsg(
                "Message exceeds the maximum allowed size".to_string(),
            ));
        }

        // Create a buffer to hold the message based on its length
        let mut payload = vec![0; header.payload_size as usize];
        self.conn.recv(&mut payload).await?;

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

        self.conn.send(&buffer).await?;
        Ok(())
    }

    /// Reads a message of type `NetMsg` with the given timeout.
    pub async fn read_timeout(&self, duration: Duration) -> Result<NetMsg> {
        timeout(duration, self.read())
            .await
            .map_err(|_| NetError::Timeout)?
    }

    /// Writes a message of type `T` with the given timeout.
    pub async fn write_timeout<T: CodecMsg>(
        &self,
        command: NetMsgCmd,
        msg: &T,
        duration: Duration,
    ) -> Result<()> {
        timeout(duration, self.write(command, msg))
            .await
            .map_err(|_| NetError::Timeout)?
    }
}
