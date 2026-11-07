use std::collections::HashMap;
use std::io::{ErrorKind, Read};

use super::bytes::Bytes;
use crate::{Error, Result};

/// Default CCSDS attached sync marker.
pub const ASM: [u8; 4] = [0x1a, 0xcf, 0xfc, 0x1d];

/// Bit-shift each byte in dat by k bits to the left, without wrapping.
fn left_shift(dat: &[u8], k: usize) -> Vec<u8> {
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

/// Number of bit-shifted variants of the sync marker, i.e., one for each bit position.
const SHIFTS: usize = 8;

/// A pattern and mask, each packed in to a big-endian word, for each shift. Patterns
/// shorter than a word are padded with zeros in both, so the padding never affects a match.
type Words = [(u64, u64); SHIFTS];

/// Pack the patterns and masks in to words, or `None` if a shifted pattern does not fit.
fn create_words(patterns: &[Vec<u8>], masks: &[Vec<u8>]) -> Option<Words> {
    let mut words = [(0u64, 0u64); SHIFTS];
    for (word, (pat, mask)) in words.iter_mut().zip(patterns.iter().zip(masks)) {
        if pat.len() > 8 {
            return None;
        }
        let mut p = [0u8; 8];
        let mut m = [0u8; 8];
        p[..pat.len()].copy_from_slice(pat);
        m[..mask.len()].copy_from_slice(mask);
        *word = (u64::from_be_bytes(p), u64::from_be_bytes(m));
    }
    Some(words)
}

/// Bit `i` of the result is set if the shift `i` pattern matches the first bytes of `w`.
#[inline]
fn word_hits(words: &Words, w: u64) -> u8 {
    let mut hits = 0u8;
    for (i, (pat, mask)) in words.iter().enumerate() {
        hits |= u8::from(w & mask == *pat) << i;
    }
    hits
}

/// A sychronized block location.
#[derive(Debug, Clone, PartialEq)]
pub struct Loc {
    /// Offset (0-based) to the first byte containing block data after a found sync marker
    pub offset: usize,
    /// The bit in the byte where the data block starts
    pub bit: u8,
}

/// Synchronizer scans a byte stream for data blocks indicated by a sync marker.
///
/// The sync marker may be bit-shifted, in which case the bytes returned will also
/// be bit shifted.
pub(crate) struct Synchronizer<R>
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
    // Patterns and masks packed in to words for fast comparison. This is `None` if the
    // marker is too long, in which case patterns and masks are compared byte by byte.
    words: Option<Words>,
    // Index of the pattern used for the last block, which is the first one tried for the
    // next block.
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
    pub fn new(reader: R, block_size: usize) -> Self {
        let (patterns, masks) = create_patterns(&ASM);
        let bytes = Bytes::new(reader);
        let words = create_words(&patterns, &masks);
        Synchronizer {
            bytes,
            block_size,
            patterns,
            masks,
            words,
            pattern_idx: 0,
            pattern_hits: HashMap::new(),
        }
    }

    /// Use the specified attached sync marker rather than the defualt ([ASM]).
    pub fn with_asm(mut self, asm: &[u8]) -> Self {
        let (patterns, masks) = create_patterns(asm);
        self.words = create_words(&patterns, &masks);
        self.patterns = patterns;
        self.masks = masks;
        self
    }

    /// True if the shift `i` pattern matches the start of `bytes`, which must have at least
    /// as many bytes as the pattern.
    fn matches(&self, i: usize, bytes: &[u8]) -> bool {
        self.patterns[i]
            .iter()
            .zip(&self.masks[i])
            .zip(bytes)
            .all(|((pat, mask), b)| b & mask == *pat)
    }

    /// Search the first `positions` byte positions of `padded` (see [`Bytes::padded`]) for
    /// a pattern. Every position must have enough bytes for the longest pattern.
    ///
    /// Positions are checked in order, and patterns in order within a position, so this
    /// returns the match with the lowest position and then the lowest shift. At the first
    /// position only the shifts from `first_shift` on are checked.
    fn search(
        &self,
        padded: &[u8],
        positions: usize,
        first_shift: usize,
    ) -> Option<(usize, usize)> {
        for p in 0..positions {
            let mut hits = match &self.words {
                Some(words) => {
                    let w = u64::from_be_bytes(padded[p..p + 8].try_into().unwrap());
                    word_hits(words, w)
                }
                None => (0..SHIFTS).fold(0u8, |hits, i| {
                    hits | (u8::from(self.matches(i, &padded[p..])) << i)
                }),
            };
            if p == 0 {
                hits &= 0xff << first_shift;
            }
            if hits != 0 {
                return Some((p, usize::try_from(hits.trailing_zeros()).unwrap()));
            }
        }
        None
    }

    /// Record a match of the shift `i` pattern, which must have just been consumed, and
    /// return its location.
    fn found(&mut self, i: usize) -> Loc {
        let shift = u8::try_from(i).unwrap();
        self.pattern_idx = i;
        self.pattern_hits
            .entry(shift)
            .and_modify(|count| *count += 1)
            .or_insert(1);
        Loc {
            // Number of bytes before the first byte holding block data, plus one. This is
            // the same for every shift.
            offset: self.bytes.offset() + 1,
            bit: (8 - shift) % 8,
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
    pub fn scan(&mut self) -> Result<Option<Loc>> {
        let n = self.patterns[0].len();
        // Bytes needed at a position to check all the patterns. Shifted patterns are one
        // byte longer than the marker.
        let need = n + 1;
        // True until a position has been completely checked. Scanning starts with the shift
        // used for the last block, and only goes back to the other shifts at later positions.
        let mut first = true;

        loop {
            let len = self.bytes.data().len();
            if len >= need {
                let positions = len - need + 1;
                let first_shift = if first { self.pattern_idx } else { 0 };
                if let Some((p, i)) = self.search(self.bytes.padded(), positions, first_shift) {
                    // Leave any partially used byte, it is the first byte of the block
                    self.bytes.consume(p + n);
                    return Ok(Some(self.found(i)));
                }
                self.bytes.consume(positions);
                first = false;
            }

            match self.bytes.refill() {
                Ok(0) => break,
                Ok(_) => {}
                Err(err) if err.kind() == ErrorKind::UnexpectedEof => break,
                Err(err) => return Err(Error::Io(err)),
            }
        }

        // End of stream. The last position has room for only the unshifted marker, and that
        // is only checked if it is the first shift to try.
        let first_shift = if first { self.pattern_idx } else { 0 };
        if first_shift == 0 && self.bytes.data().len() == n && self.matches(0, self.bytes.data()) {
            self.bytes.consume(n);
            return Ok(Some(self.found(0)));
        }
        Ok(None)
    }

    /// Fetch a block from the stream.
    ///
    /// # Errors
    /// On [Error]s filling buffer
    pub fn block(&mut self) -> Result<Vec<u8>> {
        if self.pattern_idx == 0 {
            // Byte aligned, no shifting required. If the stream ends before the block does,
            // `buf` is left as zeros.
            let mut buf = vec![0u8; self.block_size];
            self.bytes.fill(&mut buf)?;
            return Ok(buf);
        }

        // A shifted block takes bits from one more byte than its size. That byte is not
        // consumed because it is only partially used, so it is the start of the next scan.
        let size = self.block_size;
        if !self.bytes.ensure(size + 1)? {
            let remaining = self.bytes.data().len();
            self.bytes.consume(remaining);
            return Err(std::io::Error::from(ErrorKind::UnexpectedEof).into());
        }
        let k = self.pattern_idx;
        let data = self.bytes.data();
        // Each output byte is a byte shifted left with the high bits of the next byte
        // shifted in. Zipping two slices lets the compiler vectorize this.
        let buf: Vec<u8> = data[..size]
            .iter()
            .zip(&data[1..=size])
            .map(|(cur, next)| (cur << k) | (next >> (8 - k)))
            .collect();
        self.bytes.consume(size);

        Ok(buf)
    }
}

impl<R> IntoIterator for Synchronizer<R>
where
    R: Read + Send,
{
    type Item = Result<Block>;
    type IntoIter = BlockIter<R>;

    fn into_iter(self) -> Self::IntoIter {
        BlockIter {
            scanner: self,
            last: 0,
        }
    }
}

/// A block of data identified by the synchronization process.
#[derive(Debug, Clone)]
pub struct Block {
    /// Number of bytes since the start of the ASM for the last block
    pub last: usize,
    /// Where this block was found in the input stream
    pub loc: Loc,
    /// Data from the stream for this block
    pub data: Vec<u8>,
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
    last: usize,
}

impl<R> Iterator for BlockIter<R>
where
    R: Read + Send,
{
    type Item = Result<Block>;

    fn next(&mut self) -> Option<Self::Item> {
        let loc = match self.scanner.scan() {
            Ok(Some(loc)) => loc,    // got a valid Loc
            Ok(None) => return None, // no loc, must be done
            // Scan resulted in a non-EOF error, let the consumer figure out what to do
            Err(err) => return Some(Err(err)),
        };
        let last = if self.last == 0 {
            0
        } else {
            loc.offset - self.last
        };
        self.last = loc.offset;
        match self.scanner.block() {
            Ok(block) => Some(Ok(Block {
                loc,
                last,
                data: block,
            })),
            Err(err) => Some(Err(err)),
        }
    }
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
            let r = &ASM[..];
            let mut scanner = Synchronizer::new(r, 0);
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
                let mut scanner = Synchronizer::new(&pat[..], 0);
                let msg = format!("expected sync for {pat:?}");
                let loc = scanner.scan().unwrap_or_else(|_| panic!("{msg}"));

                let expected = Loc {
                    offset: 5,
                    bit: 7 - u8::try_from(i).unwrap(),
                };
                assert_eq!(loc.unwrap(), expected, "pattern {pat:?}");
            }
        }

        #[test]
        fn ccsds_asm_shifted_right_one_bit() {
            let r: &[u8] = &[13, 103, 254, 14, 128];
            let mut scanner = Synchronizer::new(r, 0);
            let loc = scanner.scan().unwrap();

            let expected = Loc { offset: 5, bit: 7 };
            assert_eq!(loc.unwrap(), expected);
        }

        #[test]
        fn block_fcn_returns_correct_bytes_with_no_shift() {
            let asm = vec![0x55];
            let r: &[u8] = &[0x55, 0x01, 0x02, 0x00, 0x00, 0x55, 0x03, 0x04, 0x00, 0x00];
            let mut scanner = Synchronizer::new(r, 2).with_asm(&asm);

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
        fn block_fcn_matches_left_shift_for_all_shifts() {
            let block_size = 16;
            // Deterministic, non-trivial bytes
            let data: Vec<u8> = (0..64u32)
                .map(|i| u8::try_from((i * 37 + 11) % 251).unwrap())
                .collect();

            for k in 0..8usize {
                let mut scanner = Synchronizer::new(&data[..], block_size);
                scanner.pattern_idx = k;

                let block = scanner.block().expect("block should succeed");

                // Reference: the original left_shift based implementation
                let want_len = if k == 0 { block_size } else { block_size + 1 };
                let expected = left_shift(&data[..want_len], k)[..block_size].to_vec();
                assert_eq!(block, expected, "shift {k}");
                assert_eq!(block.len(), block_size, "shift {k}");

                // The byte after the block must be the next one read, whether it was
                // pushed back (shifted) or never consumed (aligned).
                assert!(scanner.bytes.ensure(1).unwrap());
                assert_eq!(
                    scanner.bytes.data()[0],
                    data[block_size],
                    "next byte for shift {k}"
                );
            }
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
            let mut scanner = Synchronizer::new(r, 2).with_asm(&asm);

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

    /// Compare the synchronizer against a simple bit-by-bit model on random streams.
    mod model_tests {
        use super::*;
        use rand::{rngs::StdRng, Rng, SeedableRng};
        use std::io;

        /// Reader that returns at most `max` bytes per read, to exercise window refills.
        struct Trickle {
            dat: Vec<u8>,
            pos: usize,
            max: usize,
        }

        impl Read for Trickle {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                let n = self.max.min(buf.len()).min(self.dat.len() - self.pos);
                buf[..n].copy_from_slice(&self.dat[self.pos..self.pos + n]);
                self.pos += n;
                Ok(n)
            }
        }

        /// (offset, bit, data) for each block, or `None` where there was an error.
        type Blocks = Vec<Option<(usize, u8, Vec<u8>)>>;

        fn bits_at(dat: &[u8], start: usize, len: usize) -> Vec<bool> {
            (start..start + len)
                .map(|i| dat[i / 8] & (0x80 >> (i % 8)) != 0)
                .collect()
        }

        fn push_bytes(bits: &mut Vec<bool>, dat: &[u8]) {
            bits.extend(bits_at(dat, 0, dat.len() * 8));
        }

        fn pack(bits: &[bool]) -> Vec<u8> {
            let mut out = vec![0u8; bits.len().div_ceil(8)];
            for (i, bit) in bits.iter().enumerate() {
                if *bit {
                    out[i / 8] |= 0x80 >> (i % 8);
                }
            }
            out
        }

        /// What the synchronizer is expected to produce for `data`, worked out one bit at a
        /// time over the whole input.
        fn model(data: &[u8], asm: &[u8], block_size: usize) -> Blocks {
            let n = asm.len();
            let asm_bits = bits_at(asm, 0, n * 8);
            let mut out = Vec::new();
            // Bytes consumed, and the shift of the last block.
            let mut c = 0;
            let mut last = 0;

            loop {
                let mut found = None;
                'positions: for p in c.. {
                    // Scanning resumes with the shift of the last block
                    for i in (if p == c { last } else { 0 })..8 {
                        let len = if i == 0 { n } else { n + 1 };
                        if p + len > data.len() {
                            break 'positions;
                        }
                        if bits_at(data, p * 8 + i, n * 8) == asm_bits {
                            found = Some((p, i));
                            break 'positions;
                        }
                    }
                }
                let Some((p, i)) = found else { break };

                c = p + n;
                last = i;
                let offset = c + 1;
                let bit = u8::try_from((8 - i) % 8).unwrap();
                if i == 0 {
                    if c + block_size > data.len() {
                        // Short block comes back as zeros
                        out.push(Some((offset, bit, vec![0; block_size])));
                        c = data.len();
                    } else {
                        out.push(Some((offset, bit, data[c..c + block_size].to_vec())));
                        c += block_size;
                    }
                } else if c + block_size + 1 > data.len() {
                    out.push(None);
                    c = data.len();
                } else {
                    let bits = bits_at(data, c * 8 + i, block_size * 8);
                    out.push(Some((offset, bit, pack(&bits))));
                    c += block_size;
                }
            }
            out
        }

        fn run(data: &[u8], asm: &[u8], block_size: usize, max_read: usize) -> Blocks {
            let reader = Trickle {
                dat: data.to_vec(),
                pos: 0,
                max: max_read,
            };
            Synchronizer::new(reader, block_size)
                .with_asm(asm)
                .into_iter()
                .map(|r| r.ok().map(|b| (b.loc.offset, b.loc.bit, b.data)))
                .collect()
        }

        /// Random bits with markers and blocks at random bit positions, and sometimes cut off.
        fn make_stream(rng: &mut StdRng, asm: &[u8], block_size: usize) -> Vec<u8> {
            let mut bits = Vec::new();
            for _ in 0..rng.gen_range(0..6) {
                // Often no gap, so the shift of the last block is still correct
                if rng.gen_bool(0.5) {
                    bits.extend((0..rng.gen_range(0..40)).map(|_| rng.gen::<bool>()));
                }
                push_bytes(&mut bits, asm);
                bits.extend((0..block_size * 8).map(|_| rng.gen::<bool>()));
            }
            bits.extend((0..rng.gen_range(0..60)).map(|_| rng.gen::<bool>()));
            if !bits.is_empty() && rng.gen_bool(0.3) {
                bits.truncate(rng.gen_range(0..bits.len()));
            }
            pack(&bits)
        }

        #[test]
        fn matches_model_on_random_streams() {
            let asms: Vec<Vec<u8>> = vec![
                ASM.to_vec(),
                vec![0x55],
                // Longest marker that is compared as a word
                vec![0x1a, 0xcf, 0xfc, 0x1d, 0x03, 0x47, 0x76],
                // Shortest marker that is compared byte by byte
                vec![0x1a, 0xcf, 0xfc, 0x1d, 0x03, 0x47, 0x76, 0xc7],
                vec![0x03, 0x47, 0x76, 0xc7, 0x27, 0x28, 0x95, 0xb0, 0x1a, 0xcf],
            ];
            for seed in 0..600 {
                let mut rng = StdRng::seed_from_u64(seed);
                let asm = &asms[rng.gen_range(0..asms.len())];
                let block_size = rng.gen_range(0..24);
                let max_read = if rng.gen_bool(0.2) {
                    1 << 20
                } else {
                    rng.gen_range(1..10)
                };
                let data = make_stream(&mut rng, asm, block_size);

                assert_eq!(
                    run(&data, asm, block_size, max_read),
                    model(&data, asm, block_size),
                    "seed {seed} asm {asm:?} block_size {block_size} max_read {max_read} data {data:?}",
                );
            }
        }

        #[test]
        fn block_larger_than_window_aligned_and_shifted() {
            let block_size = 150_000;
            let mut rng = StdRng::seed_from_u64(7);
            let block: Vec<u8> = (0..block_size).map(|_| rng.gen()).collect();

            // Aligned
            let mut data = ASM.to_vec();
            data.extend(&block);
            let got = run(&data, &ASM, block_size, 1 << 20);
            assert_eq!(got, vec![Some((5, 0, block.clone()))]);

            // Shifted by 3 bits
            let mut bits = vec![true, false, true];
            push_bytes(&mut bits, &ASM);
            push_bytes(&mut bits, &block);
            // Extra byte that supplies the bits shifted in to the end of the block
            bits.extend([false; 8]);
            let got = run(&pack(&bits), &ASM, block_size, 4096);
            assert_eq!(got, vec![Some((5, 5, block))]);
        }
    }
}
