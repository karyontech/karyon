use bytes::{Buf, BytesMut};

pub type ByteBuffer = Buffer;

#[derive(Debug, Default)]
pub struct Buffer {
    inner: BytesMut,
}

impl Buffer {
    /// Constructs a new, empty Buffer.
    pub fn new() -> Self {
        Self {
            inner: BytesMut::new(),
        }
    }

    /// Returns the number of elements in the buffer.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Resizes the buffer in-place so that `len` is equal to `new_size`.
    pub fn resize(&mut self, new_size: usize) {
        self.inner.resize(new_size, 0u8);
    }

    /// Appends all elements in a slice to the buffer.
    pub fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.inner.extend_from_slice(bytes);
    }

    /// Shortens the buffer, dropping the first `cnt` bytes and keeping the
    /// rest.
    pub fn advance(&mut self, cnt: usize) {
        self.inner.advance(cnt);
    }

    /// Returns `true` if the buffer contains no elements.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
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
        assert!(!buf.is_empty());

        buf.advance(2);
        assert_eq!(&[3, 4, 5], buf.as_ref());
        assert_eq!(3, buf.len());
        assert!(!buf.is_empty());

        buf.extend_from_slice(&[6, 7, 8]);
        assert_eq!(&[3, 4, 5, 6, 7, 8], buf.as_ref());
        assert_eq!(6, buf.len());
        assert!(!buf.is_empty());

        buf.advance(4);
        assert_eq!(&[7, 8], buf.as_ref());
        assert_eq!(2, buf.len());
        assert!(!buf.is_empty());

        buf.advance(2);
        assert_eq!(&[] as &[u8], buf.as_ref());
        assert_eq!(0, buf.len());
        assert!(buf.is_empty());
    }
}
