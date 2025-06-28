pub type ByteBuffer = Buffer;

#[derive(Debug)]
pub struct Buffer {
    inner: Vec<u8>,
    len: usize,
    max_length: usize,
}

impl Buffer {
    /// Constructs a new, empty Buffer.
    pub fn new(max_length: usize) -> Self {
        Self {
            max_length,
            inner: Vec::new(),
            len: 0,
        }
    }

    /// Returns the number of elements in the buffer.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Resizes the buffer in-place so that `len` is equal to `new_size`.
    pub fn resize(&mut self, new_size: usize) {
        // Check the Buffer doesn't grow beyond its max length.
        assert!(
            self.max_length > new_size,
            "buffer resize to {} overflows the buffer max_length ({})",
            new_size,
            self.max_length
        );
        // Make sure the vector can contain the data.
        // Note 1: reserve() is a no-op if the vector capacity is already large
        // enough, but we don't want to cause the unsigned to underflow.
        // Note 2: we don't shrink the vector memory if the length is reduced,
        // as this operation might be costly and the released memory expected to
        // be small.
        // Note 3: the vector capacity (aka allocated memory) might be larger
        // than max_length due to the allocator doing over-provisioning, but it
        // is guaranteed that the data length won't overflow.
        if new_size > self.len {
            self.inner.reserve(new_size - self.len);
        }
        // This is a no-op if the new_size is greater or equal to self.len
        self.inner.truncate(new_size);
        self.len = new_size;
    }

    /// Appends all elements in a slice to the buffer.
    pub fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.resize(self.len + bytes.len());
        self.inner.extend_from_slice(bytes);
    }

    /// Shortens the buffer, dropping the first `cnt` bytes and keeping the
    /// rest.
    pub fn advance(&mut self, cnt: usize) {
        assert!(
            self.len >= cnt,
            "buffer advance of {} underflows the buffer length ({})",
            cnt,
            self.len
        );
        self.inner.rotate_left(cnt);
        self.resize(self.len - cnt);
    }

    /// Returns `true` if the buffer contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl AsMut<[u8]> for Buffer {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.inner[..self.len]
    }
}

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        &self.inner[..self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer() {
        let mut buf = Buffer::new(32);
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

    #[test]
    #[should_panic(expected = "buffer resize to 9 overflows the buffer max_length (8)")]
    fn test_buffer_resize_overflow() {
        let mut buf = Buffer::new(8);
        buf.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    #[should_panic(expected = "buffer advance of 5 underflows the buffer length (4)")]
    fn test_buffer_advance_underflow() {
        let mut buf = Buffer::new(8);
        buf.extend_from_slice(&[1, 2, 3, 4]);
        buf.advance(5);
    }
}
