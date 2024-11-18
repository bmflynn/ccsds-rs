use super::bytes::Bytes;
use crate::prelude::*;
use std::collections::HashMap;
use std::io::{ErrorKind, Read};

/// Default CCSDS attached sync marker.
pub const ASM: [u8; 4] = [0x1a, 0xcf, 0xfc, 0x1d];

/// Bit-shift each byte in dat by k bits to the left, without wrapping.
pub(crate) fn left_shift(dat: &[u8], k: usize) -> Vec<u8> {
    let mut out: Vec<u8> = vec![0; dat.len()];
    // left shift each byte the correct nufdcmber of bits
    for i in 0..dat.len() {
        out[i] = dat[i] << k;
    }
    // OR with the remainder from the i+1th byte
    if k != 0 {
        for i in 0..(dat.len() - 1) {
            out[i] |= dat[i + 1] >> (8 - k);
        }
    }
    out
}

/// Create all possible bit-shifted patterns, and their associated masks to indicate
/// significant bits, for dat.
fn create_patterns(dat: &[u8]) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut patterns: Vec<Vec<u8>> = Vec::new();
    let mut masks: Vec<Vec<u8>> = Vec::new();

    // dat padded with an extra byte to give us room to shift
    let mut padded_pattern = vec![0x0; dat.len() + 1];
    padded_pattern[1..=dat.len()].copy_from_slice(&dat[..dat.len()]);
    // for i in 1..=dat.len() {
    //     padded_pattern[i] = dat[i - 1];
    // }
    let mut padded_mask = vec![0xff; dat.len() + 1];
    padded_mask[0] = 0;

    // First pattern is just the asm (one less in length than the rest)
    patterns.push(dat.to_owned());
    // First mask is all 1s because all bits must match
    masks.push(vec![0xff; dat.len()]);

    // Bit-shift other bytes such that the first byte of the pattern is the first
    // byte of dat shifted *RIGHT* by 1.
    for i in 1..8usize {
        patterns.push(left_shift(&padded_pattern, 8 - i));
        masks.push(left_shift(&padded_mask, 8 - i));
    }

    (patterns, masks)
}

/// A sychronized block location.
#[derive(Debug, PartialEq)]
pub struct Loc {
    /// Offset (1-based) to the first byte after a found sync marker
    pub offset: usize,
    /// The bit in the byte at offset where the marker is found.
    pub bit: u8,
}

/// Synchronizer scans a byte stream for data blocks indicated by a sync marker.
///
/// The sync marker may be bit-shifted, in which case the bytes returned will also
/// be bit shifted.
pub struct Synchronizer<R>
where
    R: Read + Send,
{
    bytes: Bytes<R>,
    // Size of the block of data expected after an ASM
    block_size: usize,
    // All 8 possible bit patterns
    patterns: Vec<Vec<u8>>,
    // Bit-mask indicating the relavent bits for all 8 patterns
    masks: Vec<Vec<u8>>,
    // Index of the current pattern in the pattern vector
    pattern_idx: usize,
    /// Count of times each pattern was used.
    pub pattern_hits: HashMap<u8, i32>,
}

impl<R> Synchronizer<R>
where
    R: Read + Send,
{
    /// Creates a new ``Synchronizer``.
    ///
    /// `block_size` is the length of the CADU minus the length of the ASM.
    pub fn new(reader: R, asm: &[u8], block_size: usize) -> Self {
        let (patterns, masks) = create_patterns(asm);
        let bytes = Bytes::new(reader);
        Synchronizer {
            bytes,
            block_size,
            patterns,
            masks,
            pattern_idx: 0,
            pattern_hits: HashMap::new(),
        }
    }

    /// Scan our stream until the next sync marker is found and return a option containing
    /// a [Some(Loc)] indicating the position of the data block and any left bit-shift currently
    /// in effect. If there are not enough bytes to check the sync marker return Ok(None).
    ///
    /// # Errors
    /// On [ErrorKind::UnexpectedEof] this will return [Ok(None)]. Any other error will result
    /// in [Err(err)].
    ///
    /// # Panics
    /// On unexpected state handling bit-shifting.
    pub fn scan(&mut self) -> Result<Option<Loc>> {
        let mut b: u8 = 0;
        let mut working: Vec<u8> = Vec::new();

        'next_pattern: loop {
            for byte_idx in 0..self.patterns[self.pattern_idx].len() {
                b = match self.bytes.next() {
                    Err(err) => {
                        if err.kind() == ErrorKind::UnexpectedEof {
                            return Ok(None);
                        }
                        return Err(Error::Io(err));
                    }
                    Ok(b) => b,
                };
                working.push(b);

                if (b & self.masks[self.pattern_idx][byte_idx])
                    != self.patterns[self.pattern_idx][byte_idx]
                {
                    // No match
                    self.pattern_idx += 1;
                    if self.pattern_idx == 8 {
                        // put all but the first byte in the working set back on bytes
                        // (since we now have fully checked the first byte and know an
                        // ASM does not begin there)
                        self.pattern_idx = 0;
                        working.reverse();
                        self.bytes.push(&working[..working.len() - 1]);
                    } else {
                        // If we haven't checked all patterns put the working set back on bytes to
                        // check against the other patterns.
                        working.reverse();
                        self.bytes.push(&working);
                    }
                    working.clear();
                    continue 'next_pattern;
                }
            }

            let mut loc = Loc {
                offset: self.bytes.offset(),
                bit: (8 - u8::try_from(self.pattern_idx).unwrap()) % 8,
            };
            // Exact sync means data block starts at the next byte
            if loc.bit == 0 {
                loc.offset += 1;
            }

            if self.pattern_idx > 0 {
                self.bytes.push(&[b]);
            }

            self.pattern_hits
                .entry(u8::try_from(self.pattern_idx).unwrap())
                .and_modify(|count| *count += 1)
                .or_insert(1);

            return Ok(Some(loc));
        }
    }

    /// Fetch a block from the stream.
    ///
    /// # Errors
    /// On [Error]s filling buffer
    pub fn block(&mut self) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; self.block_size];
        if self.pattern_idx != 0 {
            // Make room for bit-shifting
            buf.push(0);
        }
        self.bytes.fill(&mut buf)?;
        if self.pattern_idx != 0 {
            // There's a partially used byte, so push it back for the next read
            self.bytes.push(&[buf[buf.len() - 1]]);
        }
        let buf = left_shift(&buf, self.pattern_idx)[..self.block_size].to_vec();

        Ok(buf)
    }
}

impl<R> IntoIterator for Synchronizer<R>
where
    R: Read + Send,
{
    type Item = Result<Vec<u8>>;
    type IntoIter = BlockIter<R>;

    fn into_iter(self) -> Self::IntoIter {
        BlockIter { scanner: self }
    }
}

/// Iterates over synchronized data in block size defined by the source [Synchronizer].
/// Created using ``Synchronizer::into_iter``.
///
/// ## Errors
/// If a full block cannot be constructed the iterator simply ends, i.e., next returns
/// `None`, however, any other error is passed on.
pub struct BlockIter<R>
where
    R: Read + Send,
{
    scanner: Synchronizer<R>,
}

impl<R> Iterator for BlockIter<R>
where
    R: Read + Send,
{
    type Item = Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.scanner.scan() {
            Ok(Some(_)) => (),       // got a valid Loc
            Ok(None) => return None, // no loc, must be done
            // Scan resulted in a non-EOF error, let the consumer figure out what to do
            Err(err) => return Some(Err(err)),
        }
        match self.scanner.block() {
            Ok(block) => Some(Ok(block)),
            Err(err) => Some(Err(err)),
        }
    }
}

/// Creates an iterator that produces byte-aligned data blocks.
///
/// `reader` is a ``std::io::Read`` implementation providing the byte stream. `asm` is the
/// attached synchronization marker used to locate blocks in the data stream, and `block_size`
/// is size of each block w/o the ASM.
///
/// The ASM need not be byte-aligned in the stream but it is expected that block data will
/// follow immediately after the ASM. Blocks returned will be byte-aligned.
///
/// Data blocks are only produced if there are `block_size` bytes available, i.e.,
/// any partial block at the end of the file is dropped.
///
/// For more control over the iteration process see [Synchronizer].
///
/// # Errors
/// Any errors reading from the stream will cause the iterator to exit.
///
pub fn read_synchronized_blocks<'a, R>(
    reader: R,
    asm: &[u8],
    block_size: usize,
) -> impl Iterator<Item = Result<Vec<u8>>> + 'a
where
    R: Read + Send + 'a,
{
    Synchronizer::new(reader, asm, block_size).into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_shift_over_asm_bytes() {
        let input = [0, 26, 207, 252, 29];
        let expected = vec![
            [0, 26, 207, 252, 29],
            [13, 103, 254, 14, 128],
            [6, 179, 255, 7, 64],
            [3, 89, 255, 131, 160],
            [1, 172, 255, 193, 208],
            [0, 214, 127, 224, 232],
            [0, 107, 63, 240, 116],
            [0, 53, 159, 248, 58],
        ];
        for i in expected.len()..0 {
            let zult = left_shift(&input[..], i);
            zult.iter().zip(expected[i]).for_each(|(x, y)| {
                assert_eq!(
                    x, &y,
                    "test:{i} expected:{expected:?} got:{zult:?} for {input:?}",
                );
            });
        }
    }

    #[test]
    fn create_patterns_over_asm_bytes() {
        let asm = ASM;
        let (patterns, _) = create_patterns(ASM.as_ref());
        for (i, x) in asm.iter().enumerate() {
            assert_eq!(patterns[0][i], *x, "missmatch at index {i}");
        }

        let expected = [
            [13, 103, 254, 14, 128],
            [6, 179, 255, 7, 64],
            [3, 89, 255, 131, 160],
            [1, 172, 255, 193, 208],
            [0, 214, 127, 224, 232],
            [0, 107, 63, 240, 116],
            [0, 53, 159, 248, 58],
        ];
        for i in 1..patterns.len() {
            assert_eq!(patterns[i], expected[i - 1]);
        }
    }

    mod scanner_tests {
        use super::*;

        #[test]
        fn ccsds_asm_with_no_bitshift_succeeds() {
            let asm = ASM.to_vec();
            let r = &ASM[..];
            let mut scanner = Synchronizer::new(r, &asm, 0);
            let loc = scanner.scan().expect("Expected scan to succeed");

            let expected = Loc { offset: 5, bit: 0 };
            assert_eq!(loc.unwrap(), expected);
        }

        #[test]
        fn ccsds_asm_shifted() {
            let patterns: Vec<[u8; 5]> = vec![
                [13, 103, 254, 14, 128],
                [6, 179, 255, 7, 64],
                [3, 89, 255, 131, 160],
                [1, 172, 255, 193, 208],
                [0, 214, 127, 224, 232],
                [0, 107, 63, 240, 116],
                [0, 53, 159, 248, 58],
            ];
            for (i, pat) in patterns.iter().enumerate() {
                let asm = ASM.to_vec();
                let mut scanner = Synchronizer::new(&pat[..], &asm, 0);
                let msg = format!("expected sync for {pat:?}");
                let loc = scanner.scan().expect(msg.as_str());

                let expected = Loc {
                    offset: 5,
                    bit: 7 - u8::try_from(i).unwrap(),
                };
                assert_eq!(loc.unwrap(), expected, "pattern {pat:?}");
            }
        }

        #[test]
        fn ccsds_asm_shifted_right_one_bit() {
            let asm = ASM.to_vec();
            let r: &[u8] = &[13, 103, 254, 14, 128];
            let mut scanner = Synchronizer::new(r, &asm, 0);
            let loc = scanner.scan().unwrap();

            let expected = Loc { offset: 5, bit: 7 };
            assert_eq!(loc.unwrap(), expected);
        }

        #[test]
        fn block_fcn_returns_correct_bytes_with_no_shift() {
            let asm = vec![0x55];
            let r: &[u8] = &[0x55, 0x01, 0x02, 0x00, 0x00, 0x55, 0x03, 0x04, 0x00, 0x00];
            let mut scanner = Synchronizer::new(r, &asm, 2);

            // First block
            let loc = scanner.scan().expect("Expected scan 1 to succeed");
            let expected = Loc { offset: 2, bit: 0 };
            assert_eq!(loc.unwrap(), expected);
            let block = scanner.block().expect("Expected block 1 to succeed");
            assert_eq!(block, [0x01, 0x02]);

            // Second block
            let loc = scanner.scan().expect("Expected scan 2 to succeed");
            let expected = Loc { offset: 7, bit: 0 };
            assert_eq!(loc.unwrap(), expected);
            let block = scanner.block().expect("Expected block 2 to succeed");
            assert_eq!(block, [0x03, 0x04]);
        }

        #[test]
        fn block_fcn_returns_correct_bytes_when_shifted_1() {
            let asm = vec![0b0101_0101];
            let r: &[u8] = &[
                0b0010_1010,
                0b1000_0000,
                0b1000_0001,
                0b0000_0000,
                0b0000_0000,
                0b0010_1010,
                0b1000_0001,
                0b1000_0010,
                0b0000_0000,
                0b0000_0000,
                0b0000_0000,
            ];
            let mut scanner = Synchronizer::new(r, &asm, 2);

            // First block
            let loc = scanner.scan().expect("Expected scan 1 to succeed");
            let expected = Loc { offset: 2, bit: 7 };
            assert_eq!(loc.unwrap(), expected);
            let block = scanner.block().expect("Expected block 1 to succeed");
            assert_eq!(block, [0x01, 0x02]);

            // Second block
            let loc = scanner.scan().expect("Expected scan 2 to succeed");
            let expected = Loc { offset: 7, bit: 7 };
            assert_eq!(loc.unwrap(), expected);
            let block = scanner.block().expect("Expected block 2 to succeed");
            assert_eq!(block, [0x03, 0x04]);
        }
    }
}
