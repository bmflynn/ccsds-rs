use std::io::{self, ErrorKind};

pub struct Bytes<R> where R: io::Read + Send {
    reader: R, 
    num_read: usize,
    cache: Vec<u8>,
    buf: [u8; 1],
}

/// Bytes provides the ability to read bytes from a reader and push them
/// back if they are not needed, i.e., Peek-and-push. The original order of
/// the bytes is preserved when pushing bytes back.
impl<R> Bytes<R> where R: io::Read + Send  {
    pub fn new(reader: R) -> Self {
        Bytes {
            reader,
            num_read: 0,
            cache: Vec::new(),
            buf: [0u8; 1],
        }
    }

    pub fn next(&mut self) -> Result<u8, io::Error> {
        if let Some(b) = self.cache.pop() {
            Ok(b)
        } else {
            let n = self.reader.read(&mut self.buf)?;
            if n == 0 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
            }
            self.num_read += 1;
            Ok(self.buf[0])
        }
    }

    pub fn fill(&mut self, buf: &mut [u8]) -> Result<bool, io::Error> {
        if self.cache.is_empty() {
            // No cache, just fill the buffer
            if let Err(err) = self.reader.read_exact(buf) {
                if err.kind() == ErrorKind::UnexpectedEof {
                    return Ok(false);
                }
                return Err(err);
            }
            self.num_read += buf.len();
            return Ok(true);
        }

        if self.cache.len() < buf.len() {
            // More bytes requested than what's in cache, fill with cache, then read
            // the rest
            buf[..self.cache.len()].clone_from_slice(&self.cache);
            self.reader.read_exact(&mut buf[self.cache.len()..])?;
            self.num_read += buf.len() - self.cache.len();
            self.cache.clear();
            return Ok(true);
        }

        // Cache contains enough bytes to fill buf
        let n = buf.len();
        buf[..].clone_from_slice(&self.cache[..n]);
        let (_, tail) = self.cache.split_at(buf.len());
        self.cache = tail.to_vec();
        Ok(true)
    }

    pub fn push(&mut self, dat: &[u8]) {
        self.cache.extend_from_slice(dat);
    }

    pub fn offset(&self) -> usize {
        self.num_read - self.cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let dat = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let mut bytes = Bytes::new(&dat[..]);

        let b = bytes
            .next()
            .expect("Should have produced a byte for first call to next");
        assert_eq!(b, 0, "first byte has bad value");
        assert_eq!(bytes.offset(), 1);

        let b = bytes
            .next()
            .expect("Should have produced a byte after second call to next");
        assert_eq!(b, 1, "second byte has bad value");
        assert_eq!(bytes.offset(), 2);

        bytes.push(&[b]);
        assert_eq!(bytes.cache, [1]);
        assert_eq!(bytes.offset(), 1);

        let b = bytes
            .next()
            .expect("Should have produced a byte after third call to next");
        assert_eq!(
            b, 1,
            "Byte should be the same as second call to next following a push"
        );
        assert_eq!(bytes.offset(), 2);
        assert_eq!(bytes.cache.len(), 0);

        let buf = &mut vec![0u8; 3][..];
        bytes.fill(buf).expect("read_exact should not have failed");
        assert_eq!(bytes.cache.len(), 0);
        assert_eq!(bytes.offset(), 5);
        assert_eq!(buf, [2, 3, 4]);
    }

    #[test]
    fn read_exact_with_no_cache() {
        let dat = [1, 2, 3, 4, 5, 6];
        let mut bytes = Bytes::new(&dat[..]);

        let buf = &mut vec![0u8; 3][..];
        bytes.fill(buf).expect("read_exact should not have failed");
        assert_eq!(buf, [1, 2, 3]);
        assert_eq!(bytes.num_read, 3);
        assert_eq!(bytes.offset(), 3);
    }

    #[test]
    fn read_exact_cache_does_not_contain_enough_bytes_to_fill_buff() {
        let dat = [1, 2, 3, 4, 5, 6];
        let mut bytes = Bytes::new(&dat[..]);

        // Read some bytes
        let buf = &mut vec![0u8; 3][..];
        bytes.fill(buf).expect("read_exact should not have failed");
        assert_eq!(buf, [1, 2, 3]);
        assert_eq!(bytes.num_read, 3);
        assert_eq!(bytes.offset(), 3);

        // Put those bytes back to cache them
        bytes.push(buf);
        assert_eq!(bytes.num_read, 3, "should have still only read 3 bytes");
        assert_eq!(
            bytes.offset(),
            0,
            "but the offset should be num_read - cache.len"
        );

        // Read again, which should produce bytes from cache + 1 read byte
        let buf = &mut vec![0u8; 4][..];
        bytes.fill(buf).expect("read_exact should not have failed");
        assert_eq!(buf, [1, 2, 3, 4]);
        assert_eq!(bytes.num_read, 4);
        assert_eq!(bytes.offset(), 4);
    }

    #[test]
    fn fill_returns_true_when_not_eof() {
        let dat: Vec<u8> = vec![1, 2, 3, 4, 5];
        let mut bytes = Bytes::new(&dat[..]);

        let buf = &mut vec![0u8; 3][..];
        let more = bytes.fill(buf).expect("should not fail");
        assert!(more, "more should be true when not EOF");
    }

    #[test]
    fn fill_returns_false_when_eof() {
        let dat: Vec<u8> = vec![];
        let mut bytes = Bytes::new(&dat[..]);

        let buf = &mut vec![0u8; 3][..];
        let more = bytes.fill(buf).expect("should not fail");
        assert!(!more, "more should be false when EOF");
    }

    #[test]
    fn read_exact_cache_contains_enough_bytes_to_fill_buff() {
        let dat = [1, 2, 3, 4, 5, 6];
        let mut bytes = Bytes::new(&dat[..]);

        let buf = &mut vec![0u8; 3][..];
        bytes.fill(buf).expect("read_exact should not have failed");
        assert_eq!(buf, [1, 2, 3]);
        assert_eq!(bytes.offset(), 3);

        // Put those bytes back to cache them
        bytes.push(buf);
        assert_eq!(bytes.offset(), 0);

        // Read again, which should product bytes from cache
        bytes.fill(buf).expect("read_exact should not have failed");
        assert_eq!(buf, [1, 2, 3]);
        assert_eq!(bytes.offset(), 3);
    }
}
