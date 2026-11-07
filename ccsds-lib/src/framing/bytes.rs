use std::io::{self, ErrorKind};

/// Number of bytes read from the reader at a time, unless a larger window is needed.
const CHUNK: usize = 64 * 1024;

/// Number of readable bytes kept after the end of the valid data. See [`Bytes::padded`].
pub const PAD: usize = 8;

/// Bytes keeps a contiguous window of data read from a reader.
///
/// Callers look at the unconsumed data with [`Bytes::data`], ask for more with
/// [`Bytes::refill`] or [`Bytes::ensure`], and drop data they are finished with using
/// [`Bytes::consume`]. Data is never pushed back; unconsumed data stays in the window.
pub struct Bytes<R>
where
    R: io::Read + Send,
{
    reader: R,
    // Always `PAD` bytes longer than the space available for data.
    buf: Vec<u8>,
    // Start of the unconsumed data in `buf`.
    pos: usize,
    // End of the valid data in `buf`.
    end: usize,
    // Number of bytes consumed since the start of the stream.
    consumed: usize,
}

impl<R> Bytes<R>
where
    R: io::Read + Send,
{
    pub fn new(reader: R) -> Self {
        Bytes {
            reader,
            buf: vec![0u8; CHUNK + PAD],
            pos: 0,
            end: 0,
            consumed: 0,
        }
    }

    /// Unconsumed bytes in the window.
    pub fn data(&self) -> &[u8] {
        &self.buf[self.pos..self.end]
    }

    /// Same as [`Bytes::data`] but followed by `PAD` more readable bytes.
    ///
    /// The extra bytes are not part of the stream and have no defined value. This lets a
    /// caller load a fixed size word at any index of `data` without a bounds special case,
    /// as long as it ignores the bytes it does not need.
    pub fn padded(&self) -> &[u8] {
        &self.buf[self.pos..self.end + PAD]
    }

    /// Drop the first `n` unconsumed bytes.
    ///
    /// # Panics
    /// If `n` is more than the number of unconsumed bytes.
    pub fn consume(&mut self, n: usize) {
        assert!(n <= self.end - self.pos, "consume past end of data");
        self.pos += n;
        self.consumed += n;
    }

    /// Number of bytes consumed since the start of the stream.
    pub fn offset(&self) -> usize {
        self.consumed
    }

    /// Read more data from the reader to the end of the window and return the number of
    /// bytes added, which is 0 at the end of the stream.
    ///
    /// Consumed bytes are dropped to make room, so a slice from [`Bytes::data`] must not
    /// be kept across this call.
    pub fn refill(&mut self) -> io::Result<usize> {
        if self.pos > 0 {
            self.buf.copy_within(self.pos..self.end, 0);
            self.end -= self.pos;
            self.pos = 0;
        }
        if self.end == self.buf.len() - PAD {
            // Window is full of unconsumed data, make it bigger.
            let new_len = self.buf.len() * 2;
            self.buf.resize(new_len, 0);
        }
        let space = self.buf.len() - PAD;
        loop {
            match self.reader.read(&mut self.buf[self.end..space]) {
                Ok(n) => {
                    self.end += n;
                    return Ok(n);
                }
                Err(err) if err.kind() == ErrorKind::Interrupted => {}
                Err(err) => return Err(err),
            }
        }
    }

    /// Read until there are at least `n` unconsumed bytes. Returns false if the stream
    /// ended first.
    pub fn ensure(&mut self, n: usize) -> io::Result<bool> {
        while self.end - self.pos < n {
            if self.refill()? == 0 {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Consume exactly `buf.len()` bytes into `buf`.
    ///
    /// If the stream ends first this returns `Ok(false)`, leaves `buf` unchanged, and
    /// consumes all remaining data.
    pub fn fill(&mut self, buf: &mut [u8]) -> io::Result<bool> {
        if !self.ensure(buf.len())? {
            self.consume(self.end - self.pos);
            return Ok(false);
        }
        buf.copy_from_slice(&self.data()[..buf.len()]);
        self.consume(buf.len());
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reader that returns at most `max` bytes per read
    struct Trickle<'a> {
        dat: &'a [u8],
        max: usize,
    }

    impl io::Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.max.min(buf.len()).min(self.dat.len());
            buf[..n].copy_from_slice(&self.dat[..n]);
            self.dat = &self.dat[n..];
            Ok(n)
        }
    }

    #[test]
    fn data_is_empty_before_first_refill() {
        let dat = [1, 2, 3];
        let bytes = Bytes::new(&dat[..]);
        assert!(bytes.data().is_empty());
        assert_eq!(bytes.offset(), 0);
    }

    #[test]
    fn refill_consume_and_offset() {
        let dat = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let mut bytes = Bytes::new(&dat[..]);

        assert_eq!(bytes.refill().unwrap(), 10);
        assert_eq!(bytes.data(), dat);
        bytes.consume(2);
        assert_eq!(bytes.data(), &dat[2..]);
        assert_eq!(bytes.offset(), 2);

        assert_eq!(bytes.refill().unwrap(), 0, "expected end of stream");
        assert_eq!(bytes.data(), &dat[2..]);
    }

    #[test]
    fn padded_is_data_followed_by_pad_bytes() {
        let dat = [1, 2, 3];
        let mut bytes = Bytes::new(&dat[..]);
        bytes.refill().unwrap();
        assert_eq!(bytes.padded().len(), 3 + PAD);
        assert_eq!(&bytes.padded()[..3], dat);
    }

    #[test]
    fn ensure_reads_until_enough_bytes() {
        let dat: Vec<u8> = (0..20).collect();
        let mut bytes = Bytes::new(Trickle { dat: &dat, max: 3 });

        assert!(bytes.ensure(10).unwrap());
        assert!(bytes.data().len() >= 10);
        assert_eq!(&bytes.data()[..10], &dat[..10]);

        assert!(!bytes.ensure(21).unwrap(), "only 20 bytes in stream");
        assert_eq!(bytes.data(), dat);
    }

    #[test]
    fn fill_copies_and_consumes() {
        let dat = [1, 2, 3, 4, 5, 6];
        let mut bytes = Bytes::new(&dat[..]);

        let buf = &mut [0u8; 3][..];
        assert!(bytes.fill(buf).unwrap());
        assert_eq!(buf, [1, 2, 3]);
        assert_eq!(bytes.offset(), 3);

        assert!(bytes.fill(buf).unwrap());
        assert_eq!(buf, [4, 5, 6]);
        assert_eq!(bytes.offset(), 6);
    }

    #[test]
    fn fill_returns_false_and_drains_at_end_of_stream() {
        let dat = [1, 2];
        let mut bytes = Bytes::new(&dat[..]);

        let buf = &mut [9u8; 3][..];
        assert!(!bytes.fill(buf).unwrap());
        assert_eq!(buf, [9, 9, 9], "buf should be unchanged");
        assert!(bytes.data().is_empty(), "remaining data should be consumed");
        assert_eq!(bytes.offset(), 2);
    }

    #[test]
    fn fill_returns_false_for_empty_stream() {
        let dat: Vec<u8> = vec![];
        let mut bytes = Bytes::new(&dat[..]);
        let buf = &mut [0u8; 3][..];
        assert!(!bytes.fill(buf).unwrap());
    }

    #[test]
    fn window_grows_to_hold_more_than_one_chunk() {
        let dat: Vec<u8> = (0..(CHUNK * 2 + 5)).map(|i| (i % 251) as u8).collect();
        let mut bytes = Bytes::new(&dat[..]);

        assert!(bytes.ensure(dat.len()).unwrap());
        assert_eq!(bytes.data(), dat);

        // Data stays correct after consuming and compacting
        bytes.consume(CHUNK + 1);
        assert_eq!(bytes.refill().unwrap(), 0);
        assert_eq!(bytes.data(), &dat[CHUNK + 1..]);
    }
}
