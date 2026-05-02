use std::io::{self, ErrorKind};

use karyon_core::async_runtime::io::{split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};

use crate::{
    codec::Codec,
    message::{MessageRx, MessageTx},
    stream::ByteStream,
    Buffer, ByteBuffer, Endpoint, Result,
};

// Default read chunk size (64KB).
const DEFAULT_READ_CHUNK_SIZE: usize = 64 * 1024;

/// Convert a ByteStream + codec into a FramedConn.
/// The codec handles both framing and serialization over the
/// byte stream. Call `split()` for concurrent read/write.
pub fn framed<C>(stream: Box<dyn ByteStream>, codec: C) -> FramedConn<C>
where
    C: Codec<ByteBuffer> + Clone + 'static,
    C::Error: From<io::Error> + Into<crate::Error> + Send + Sync,
    C::Message: Send + Sync + 'static,
{
    let peer = stream.peer_endpoint();
    let local = stream.local_endpoint();
    let (rh, wh) = split(stream);

    FramedConn {
        reader: FramedReader {
            inner: rh,
            decoder: codec.clone(),
            buffer: Buffer::new(),
            read_chunk_size: DEFAULT_READ_CHUNK_SIZE,
            peer_endpoint: peer.clone(),
            local_endpoint: local.clone(),
        },
        writer: FramedWriter {
            inner: wh,
            encoder: codec,
            buffer: Buffer::new(),
            peer_endpoint: peer,
            local_endpoint: local,
        },
    }
}

/// Framed message connection over a byte stream.
pub struct FramedConn<C> {
    reader: FramedReader<C>,
    writer: FramedWriter<C>,
}

impl<C> FramedConn<C>
where
    C: Codec<ByteBuffer> + Clone + Send + Sync + 'static,
    C::Message: Send + Sync + 'static,
    C::Error: From<io::Error> + Into<crate::Error> + Send + Sync,
{
    /// Receive one complete message.
    pub async fn recv_msg(&mut self) -> Result<C::Message> {
        self.reader.recv_msg().await
    }

    /// Send one complete message.
    pub async fn send_msg(&mut self, msg: C::Message) -> Result<()> {
        self.writer.send_msg(msg).await
    }

    /// Remote peer address.
    pub fn peer_endpoint(&self) -> Option<Endpoint> {
        self.reader.peer_endpoint.clone()
    }

    /// Local address.
    pub fn local_endpoint(&self) -> Option<Endpoint> {
        self.reader.local_endpoint.clone()
    }

    /// Split into independent reader and writer halves.
    pub fn split(self) -> (FramedReader<C>, FramedWriter<C>) {
        (self.reader, self.writer)
    }
}

/// Read half of a framed connection.
pub struct FramedReader<C> {
    inner: ReadHalf<Box<dyn ByteStream>>,
    decoder: C,
    buffer: ByteBuffer,
    read_chunk_size: usize,
    peer_endpoint: Option<Endpoint>,
    local_endpoint: Option<Endpoint>,
}

impl<C> FramedReader<C>
where
    C: Codec<ByteBuffer> + Send + Sync,
    C::Message: Send + Sync,
    C::Error: From<io::Error> + Into<crate::Error> + Send + Sync,
{
    /// Remote peer address.
    pub fn peer_endpoint(&self) -> Option<Endpoint> {
        self.peer_endpoint.clone()
    }

    /// Local address.
    pub fn local_endpoint(&self) -> Option<Endpoint> {
        self.local_endpoint.clone()
    }

    /// Receive one complete message.
    pub async fn recv_msg(&mut self) -> Result<C::Message> {
        // Try decoding from buffered data first.
        if let Some((n, item)) = self.decoder.decode(&mut self.buffer).map_err(Into::into)? {
            self.buffer.advance(n);
            return Ok(item);
        }

        loop {
            let mut buf = vec![0u8; self.read_chunk_size];
            let n = self.inner.read(&mut buf).await?;

            if n == 0 {
                if self.buffer.is_empty() {
                    return Err(io::Error::from(ErrorKind::ConnectionAborted).into());
                } else {
                    return Err(io::Error::new(
                        ErrorKind::UnexpectedEof,
                        "bytes remaining in buffer",
                    )
                    .into());
                }
            }

            self.buffer.extend_from_slice(&buf[..n]);

            if let Some((cn, item)) = self.decoder.decode(&mut self.buffer).map_err(Into::into)? {
                self.buffer.advance(cn);
                return Ok(item);
            }
        }
    }
}

impl<C> MessageRx for FramedReader<C>
where
    C: Codec<ByteBuffer> + Send + Sync,
    C::Message: Send + Sync,
    C::Error: From<io::Error> + Into<crate::Error> + Send + Sync,
{
    type Message = C::Message;

    fn recv_msg(&mut self) -> impl std::future::Future<Output = Result<Self::Message>> + Send {
        FramedReader::recv_msg(self)
    }

    fn peer_endpoint(&self) -> Option<Endpoint> {
        FramedReader::peer_endpoint(self)
    }
}

/// Write half of a framed connection.
pub struct FramedWriter<C> {
    inner: WriteHalf<Box<dyn ByteStream>>,
    encoder: C,
    buffer: ByteBuffer,
    peer_endpoint: Option<Endpoint>,
    local_endpoint: Option<Endpoint>,
}

impl<C> FramedWriter<C>
where
    C: Codec<ByteBuffer> + Send + Sync,
    C::Message: Send + Sync,
    C::Error: From<io::Error> + Into<crate::Error> + Send + Sync,
{
    /// Remote peer address.
    pub fn peer_endpoint(&self) -> Option<Endpoint> {
        self.peer_endpoint.clone()
    }

    /// Local address.
    pub fn local_endpoint(&self) -> Option<Endpoint> {
        self.local_endpoint.clone()
    }

    /// Send one complete message.
    pub async fn send_msg(&mut self, msg: C::Message) -> Result<()> {
        self.encoder
            .encode(&msg, &mut self.buffer)
            .map_err(Into::into)?;

        while !self.buffer.is_empty() {
            let n = self.inner.write(self.buffer.as_ref()).await?;
            if n == 0 {
                return Err(io::Error::from(ErrorKind::UnexpectedEof).into());
            }
            self.buffer.advance(n);
        }

        self.inner.flush().await?;
        Ok(())
    }
}

impl<C> MessageTx for FramedWriter<C>
where
    C: Codec<ByteBuffer> + Send + Sync,
    C::Message: Send + Sync,
    C::Error: From<io::Error> + Into<crate::Error> + Send + Sync,
{
    type Message = C::Message;

    fn send_msg(
        &mut self,
        msg: Self::Message,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
        FramedWriter::send_msg(self, msg)
    }

    fn peer_endpoint(&self) -> Option<Endpoint> {
        FramedWriter::peer_endpoint(self)
    }
}
