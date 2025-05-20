pub type ByteBuffer = Buffer<Vec<u8>>;

#[derive(Debug)]
pub struct Buffer<B> {
    inner: B,
    len: usize,
    cap: usize,
}

impl<B> Buffer<B>
where
    B: AsMut<[u8]> + AsRef<[u8]>,
{
    /// Constructs a new, empty Buffer<B>.
    pub fn new(b: B) -> Self {
        Self {
            cap: b.as_ref().len(),
            inner: b,
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
        assert!(self.cap > new_size);
        self.len = new_size;
    }

    /// Appends all elements in a slice to the buffer.
    pub fn extend_from_slice(&mut self, bytes: &[u8]) {
        let old_len = self.len;
        self.resize(self.len + bytes.len());
        self.inner.as_mut()[old_len..bytes.len() + old_len].copy_from_slice(bytes);
    }

    /// Shortens the buffer, dropping the first `cnt` bytes and keeping the
    /// rest.
    pub fn advance(&mut self, cnt: usize) {
        assert!(self.len >= cnt);
        self.inner.as_mut().rotate_left(cnt);
        self.resize(self.len - cnt);
    }

    /// Returns `true` if the buffer contains no elements.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<B> AsMut<[u8]> for Buffer<B>
where
    B: AsMut<[u8]> + AsRef<[u8]>,
{
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.inner.as_mut()[..self.len]
    }
}

impl<B> AsRef<[u8]> for Buffer<B>
where
    B: AsMut<[u8]> + AsRef<[u8]>,
{
    fn as_ref(&self) -> &[u8] {
        &self.inner.as_ref()[..self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_advance() {
        let mut buf = Buffer::new([0u8; 32]);
        buf.extend_from_slice(&[1, 2, 3]);
        assert_eq!([1, 2, 3], buf.as_ref());
    }
}
