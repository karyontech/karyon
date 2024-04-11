mod buffer;
mod websocket;

pub use websocket::WsStream;

use std::{
    io::ErrorKind,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{
    ready,
    stream::{Stream, StreamExt},
    Sink,
};
use pin_project_lite::pin_project;

use karyon_core::async_runtime::io::{AsyncRead, AsyncWrite};

use crate::{
    codec::{Decoder, Encoder},
    Error, Result,
};

use buffer::Buffer;

const BUFFER_SIZE: usize = 2048 * 2024; // 4MB
const INITIAL_BUFFER_SIZE: usize = 1024 * 1024; // 1MB

pub struct ReadStream<T, C> {
    inner: T,
    decoder: C,
    buffer: Buffer<[u8; BUFFER_SIZE]>,
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
            buffer: Buffer::new([0u8; BUFFER_SIZE]),
        }
    }

    pub async fn recv(&mut self) -> Result<C::DeItem> {
        match self.next().await {
            Some(m) => m,
            None => Err(Error::IO(std::io::ErrorKind::ConnectionAborted.into())),
        }
    }
}

pin_project! {
    pub struct WriteStream<T, C> {
        #[pin]
        inner: T,
        encoder: C,
        high_water_mark: usize,
        buffer: Buffer<[u8; BUFFER_SIZE]>,
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
            buffer: Buffer::new([0u8; BUFFER_SIZE]),
        }
    }
}

impl<T, C> Stream for ReadStream<T, C>
where
    T: AsyncRead + Unpin,
    C: Decoder + Unpin,
{
    type Item = Result<C::DeItem>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        if let Some((n, item)) = this.decoder.decode(this.buffer.as_mut())? {
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

            match this.decoder.decode(this.buffer.as_mut())? {
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

impl<T, C> Sink<C::EnItem> for WriteStream<T, C>
where
    T: AsyncWrite + Unpin,
    C: Encoder + Unpin,
{
    type Error = Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<()>> {
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

    fn start_send(mut self: Pin<&mut Self>, item: C::EnItem) -> Result<()> {
        let this = &mut *self;
        let mut buf = [0u8; INITIAL_BUFFER_SIZE];
        let n = this.encoder.encode(&item, &mut buf)?;
        this.buffer.extend_from_slice(&buf[..n]);
        Ok(())
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        ready!(self.as_mut().poll_ready(cx))?;
        self.project().inner.poll_flush(cx).map_err(Into::into)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        ready!(self.as_mut().poll_flush(cx))?;
        #[cfg(feature = "smol")]
        return self.project().inner.poll_close(cx).map_err(Error::from);
        #[cfg(feature = "tokio")]
        return self.project().inner.poll_shutdown(cx).map_err(Error::from);
    }
}
