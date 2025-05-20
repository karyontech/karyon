#[cfg(feature = "ws")]
mod websocket;

#[cfg(feature = "ws")]
pub use websocket::{ReadWsStream, WriteWsStream, WsStream};

use std::{
    io::ErrorKind,
    pin::Pin,
    result::Result,
    task::{Context, Poll},
};

use futures_util::{
    ready,
    stream::{Stream, StreamExt},
    Sink,
};
use pin_project_lite::pin_project;

use karyon_core::async_runtime::io::{AsyncRead, AsyncWrite};

use crate::codec::{Buffer, ByteBuffer, Decoder, Encoder};

const BUFFER_SIZE: usize = 4096 * 4096; // 16MB
const INITIAL_BUFFER_SIZE: usize = 1024 * 1024; // 1MB

pub struct ReadStream<T, C> {
    inner: T,
    decoder: C,
    buffer: ByteBuffer,
}

impl<T, C> ReadStream<T, C>
where
    T: AsyncRead + Unpin,
    C: Decoder + Unpin,
{
    pub fn new(inner: T, decoder: C) -> Self {
        Self {
            inner,
            decoder,
            buffer: Buffer::new(vec![0u8; BUFFER_SIZE]),
        }
    }

    pub async fn recv(&mut self) -> Result<C::DeMessage, C::DeError> {
        match self.next().await {
            Some(m) => m,
            None => Err(std::io::Error::from(std::io::ErrorKind::ConnectionAborted).into()),
        }
    }
}

pin_project! {
    pub struct WriteStream<T, C> {
        #[pin]
        inner: T,
        encoder: C,
        high_water_mark: usize,
        buffer: ByteBuffer,
    }
}

impl<T, C> WriteStream<T, C>
where
    T: AsyncWrite + Unpin,
    C: Encoder + Unpin,
{
    pub fn new(inner: T, encoder: C) -> Self {
        Self {
            inner,
            encoder,
            high_water_mark: 131072,
            buffer: Buffer::new(vec![0u8; BUFFER_SIZE]),
        }
    }
}

impl<T, C> Stream for ReadStream<T, C>
where
    T: AsyncRead + Unpin,
    C: Decoder + Unpin,
{
    type Item = Result<C::DeMessage, C::DeError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        if let Some((n, item)) = this.decoder.decode(&mut this.buffer)? {
            this.buffer.advance(n);
            return Poll::Ready(Some(Ok(item)));
        }

        let mut buf = [0u8; INITIAL_BUFFER_SIZE];
        #[cfg(feature = "tokio")]
        let mut buf = tokio::io::ReadBuf::new(&mut buf);

        loop {
            #[cfg(feature = "smol")]
            let n = ready!(Pin::new(&mut this.inner).poll_read(cx, &mut buf))?;
            #[cfg(feature = "smol")]
            let bytes = &buf[..n];

            #[cfg(feature = "tokio")]
            ready!(Pin::new(&mut this.inner).poll_read(cx, &mut buf))?;
            #[cfg(feature = "tokio")]
            let bytes = buf.filled();
            #[cfg(feature = "tokio")]
            let n = bytes.len();

            this.buffer.extend_from_slice(bytes);

            #[cfg(feature = "tokio")]
            buf.clear();

            match this.decoder.decode(&mut this.buffer)? {
                Some((cn, item)) => {
                    this.buffer.advance(cn);
                    return Poll::Ready(Some(Ok(item)));
                }
                None if n == 0 => {
                    if this.buffer.is_empty() {
                        return Poll::Ready(None);
                    } else {
                        return Poll::Ready(Some(Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "bytes remaining in read stream",
                        )
                        .into())));
                    }
                }
                _ => continue,
            }
        }
    }
}

impl<T, C> Sink<C::EnMessage> for WriteStream<T, C>
where
    T: AsyncWrite + Unpin,
    C: Encoder + Unpin,
{
    type Error = C::EnError;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        let this = &mut *self;
        while !this.buffer.is_empty() {
            let n = ready!(Pin::new(&mut this.inner).poll_write(cx, this.buffer.as_ref()))?;

            if n == 0 {
                return Poll::Ready(Err(std::io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "End of file",
                )
                .into()));
            }

            this.buffer.advance(n);
        }

        Poll::Ready(Ok(()))
    }

    fn start_send(mut self: Pin<&mut Self>, item: C::EnMessage) -> Result<(), Self::Error> {
        let this = &mut *self;
        this.encoder.encode(&item, &mut this.buffer)?;
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().poll_ready(cx))?;
        self.project().inner.poll_flush(cx).map_err(Into::into)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().poll_flush(cx))?;
        #[cfg(feature = "smol")]
        return self.project().inner.poll_close(cx).map_err(|e| e.into());

        #[cfg(feature = "tokio")]
        return self.project().inner.poll_shutdown(cx).map_err(|e| e.into());
    }
}
