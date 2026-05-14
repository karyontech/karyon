use bytes::{Buf, BytesMut};

/// Back-compat alias used as the wire type in `Codec<W>` for byte-stream
/// framing.
pub type ByteBuffer = Buffer;

/// Mutable, growable byte container. Used by codecs to build and consume
/// framed payloads, and as an owned container for transport-level payloads.
#[derive(Debug, Default)]
pub struct Buffer {
    inner: BytesMut,
}

impl Buffer {
    /// Constructs a new, empty `Buffer`.
    pub fn new() -> Self {
        Self {
            inner: BytesMut::new(),
        }
    }

    /// Build a `Buffer` from a slice (copies).
    pub fn from_slice(data: &[u8]) -> Self {
        let mut b = Self::new();
        b.extend_from_slice(data);
        b
    }

    /// Number of bytes in the buffer.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Resize the buffer in-place to `new_size` (filling with zeros if growing).
    pub fn resize(&mut self, new_size: usize) {
        self.inner.resize(new_size, 0u8);
    }

    /// Append a slice.
    pub fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.inner.extend_from_slice(bytes);
    }

    /// Append bytes from another `Buffer`.
    pub fn extend(&mut self, other: &Buffer) {
        self.inner.extend(other.as_ref());
    }

    /// Drop the first `cnt` bytes, keeping the rest.
    pub fn advance(&mut self, cnt: usize) {
        self.inner.advance(cnt);
    }

    /// Returns `true` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Freeze into an immutable `Bytes`. Zero-copy — transfers ownership of
    /// the underlying allocation.
    pub fn freeze(self) -> Bytes {
        Bytes {
            inner: self.inner.freeze(),
        }
    }
}

impl AsMut<[u8]> for Buffer {
    fn as_mut(&mut self) -> &mut [u8] {
        self.inner.as_mut()
    }
}

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        self.inner.as_ref()
    }
}

/// Immutable, cheaply cloneable byte container. .
#[derive(Debug, Clone, Default)]
pub struct Bytes {
    inner: bytes::Bytes,
}

impl Bytes {
    /// Build from a slice (copies).
    pub fn from_slice(data: &[u8]) -> Self {
        Self {
            inner: bytes::Bytes::copy_from_slice(data),
        }
    }

    /// Number of bytes.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Consume into the underlying `bytes::Bytes`. For interop with crates
    /// that speak the `bytes` API natively.
    #[cfg(feature = "quic")]
    pub(crate) fn into_inner(self) -> bytes::Bytes {
        self.inner
    }

    /// Wrap an existing `bytes::Bytes` without copying.
    #[cfg(feature = "quic")]
    pub(crate) fn from_inner(inner: bytes::Bytes) -> Self {
        Self { inner }
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        self.inner.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer() {
        let mut buf = Buffer::new();
        assert_eq!(&[] as &[u8], buf.as_ref());
        assert_eq!(0, buf.len());
        assert!(buf.is_empty());

        buf.extend_from_slice(&[1, 2, 3, 4, 5]);
        assert_eq!(&[1, 2, 3, 4, 5], buf.as_ref());
        assert_eq!(5, buf.len());

        buf.advance(2);
        assert_eq!(&[3, 4, 5], buf.as_ref());

        buf.extend_from_slice(&[6, 7, 8]);
        assert_eq!(&[3, 4, 5, 6, 7, 8], buf.as_ref());

        buf.advance(4);
        assert_eq!(&[7, 8], buf.as_ref());

        buf.advance(2);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_freeze_and_bytes() {
        let mut buf = Buffer::new();
        buf.extend_from_slice(b"hello");
        let frozen = buf.freeze();
        assert_eq!(frozen.as_ref(), b"hello");
        assert_eq!(frozen.len(), 5);
        assert!(!frozen.is_empty());

        let b = Bytes::from_slice(b"world");
        assert_eq!(b.as_ref(), b"world");
    }
}
