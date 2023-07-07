use std::{
    collections::HashMap,
    io,
};
use thiserror::Error;

use crate::bytes::Bytes;

pub const ASM: [u8; 4] = [0x1a, 0xcf, 0xfc, 0x1d];

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("IO error")]
    IO(#[from] io::Error),
    #[error("EOF")]
    EOF,
}

/// Bit-shift each byte in dat by k bits to the left, without wrapping.
pub fn left_shift(dat: &Vec<u8>, k: u8) -> Vec<u8> {
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
fn create_patterns(dat: &Vec<u8>) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut patterns: Vec<Vec<u8>> = Vec::new();
    let mut masks: Vec<Vec<u8>> = Vec::new();

    // dat padded with an extra byte to give us room to shift
    let mut padded_pattern = vec![0x0; dat.len() + 1];
    for i in 1..dat.len() + 1 {
        padded_pattern[i] = dat[i - 1];
    }
    let mut padded_mask = vec![0xff; dat.len() + 1];
    padded_mask[0] = 0;

    // First pattern is just the asm (one less in length than the rest)
    patterns.push(dat.to_vec());
    // First mask is all 1s because all bits must match
    masks.push(vec![0xff; dat.len()]);

    // Bit-shift other bytes such that the first byte of the pattern is the first
    // byte of dat shifted *RIGHT* by 1.
    for i in 1..8u8 {
        patterns.push(left_shift(&padded_pattern, 8 - i));
        masks.push(left_shift(&padded_mask, 8 - i));
    }

    (patterns, masks)
}

#[derive(Debug, PartialEq)]
pub struct Loc {
    /// Offset (1-based) to the first byte of a found sync marker that contains any
    /// relavant bits.
    pub offset: usize,
    /// The bit in the byte at offset where the marker is found.
    pub bit: u8,
}

// Synchronizer scans a byte stream for data blocks indicated by a sync marker. 
//
// The sync marker may be bit-shifted, in which case the bytes returned will also
// be bit shifted.
pub struct Synchronizer<'a> {
    bytes: Bytes<'a>,
    // Size of the block of data expected after an ASM
    block_size: i32,
    // All 8 possible bit patterns
    patterns: Vec<Vec<u8>>,
    // Bit-mask indicating the relavent bits for all 8 patterns
    masks: Vec<Vec<u8>>,
    // Index of the current pattern in the pattern vector
    pattern_idx: usize,

    pub pattern_hits: HashMap<u8, i32>,
}

impl<'a> Synchronizer<'a> {
    pub fn new(reader: impl io::Read + 'a, asm: &Vec<u8>, block_size: i32) -> Self {
        let (patterns, masks) = create_patterns(&asm);
        let bytes = Bytes::new(io::BufReader::new(reader));
        Synchronizer {
            bytes,
            block_size,
            patterns,
            masks,
            pattern_idx: 0,
            pattern_hits: HashMap::new(),
        }
    }

    /// Scan our stream until the next sync marker is found and return a option conatining
    /// a Some(Loc) indicating the position of the data block and any left bit-shift currenty
    /// in effect. If there are not enough bytes to check the sync marker return Ok(None).
    /// Any io errors other than EOF will result in an Error.
    pub fn scan(&mut self) -> Result<Loc, SyncError> {
        let mut b: u8 = 0;
        let mut working: Vec<u8> = Vec::new();

        'next_pattern: loop {
            for byte_idx in 0..self.patterns[self.pattern_idx].len() {
                b = self.bytes.next()?;
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
                        self.bytes.push(&working[..working.len()-1]);
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
                bit: (8 - self.pattern_idx as u8) % 8,
            };
            // Exact sync means data block starts at the next byte
            if loc.bit == 0 {
                loc.offset += 1;
            }

            if self.pattern_idx > 0 {
                self.bytes.push(&[b]);
            }

            self.pattern_hits
                .entry(self.pattern_idx as u8)
                .and_modify(|count| *count += 1)
                .or_insert(1);

            return Ok(loc);
        }
    }

    pub fn block(&mut self) -> Result<Vec<u8>, SyncError> {
        let mut buf = vec![0u8; self.block_size as usize];
        if self.pattern_idx != 0 {
            // Make room for bit-shifting
            buf.push(0);
        }
        self.bytes.read_exact(&mut buf)?;
        if self.pattern_idx != 0 {
            // There's a partially used byte, so push it back for the next read
            self.bytes.push(&[buf[buf.len() - 1]]);
        }
        let buf = left_shift(&buf, self.pattern_idx as u8)[..self.block_size as usize].to_vec();

        return Ok(buf);
    }
}

impl <'a> IntoIterator for Synchronizer<'a> {
    type Item = Result<Vec<u8>, Box<dyn std::error::Error>>;
    type IntoIter = BlockIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        BlockIter{scanner: self}
    }
} 

pub struct BlockIter<'a> {
    scanner: Synchronizer<'a>,
}

impl<'a> Iterator for BlockIter<'_> {
    type Item = Result<Vec<u8>, Box<dyn std::error::Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Err(err) = self.scanner.scan() {
            match err {
                SyncError::EOF => return None,
                _ => return Some(Err(Box::new(err))),
            }
        };

        match self.scanner.block() {
            Ok(block) => Some(Ok(block)),
            Err(err) => {
                match err {
                    SyncError::EOF => return None,
                    _ => return Some(Err(Box::new(err))),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
            let zult = left_shift(&input[..].to_vec(), i as u8);
            zult.iter().zip(expected[i]).for_each(|(x, y)| {
                assert_eq!(
                    x, &y,
                    "test:{} expected:{:?} got:{:?} for {:?}",
                    i, expected, zult, input
                );
            });
        }
    }

    #[test]
    fn create_patterns_over_asm_bytes() {
        let asm = ASM;
        let (patterns, _) = create_patterns(&ASM.to_vec());
        for i in 0..asm.len() {
            assert_eq!(patterns[0][i], asm[i], "missmatch at index {}", i);
        }

        let expected = vec![
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
            // let r = fs::File::open("../dldecode/testdata/overpass_snpp_2017_7min.dat").unwrap();
            let mut scanner = Synchronizer::new(r, &asm, 0);
            let loc = scanner.scan().expect("Expected scan to succeed");

            let expected = Loc { offset: 5, bit: 0 };
            assert_eq!(loc, expected);
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
                let msg = format!("expected sync for {:?}", pat);
                let loc = scanner.scan().expect(msg.as_str());

                let expected = Loc {
                    offset: 5,
                    bit: 7 - i as u8,
                };
                assert_eq!(loc, expected, "pattern {:?}", pat);
            }
        }

        #[test]
        fn ccsds_asm_shifted_right_one_bit() {
            let asm = ASM.to_vec();
            let r: &[u8] = &[13, 103, 254, 14, 128];
            let mut scanner = Synchronizer::new(r, &asm, 0);
            let loc = scanner.scan().unwrap();

            let expected = Loc { offset: 5, bit: 7 };
            assert_eq!(loc, expected);
        }

        #[test]
        #[ignore]
        fn finds_first_sync_marker_in_overpass_file() {
            let asm = ASM.to_vec();
            let r = fs::File::open("../dldecode/testdata/overpass_snpp_2017_7min.dat").unwrap();
            let mut scanner = Synchronizer::new(r, &asm, 0);
            let loc = scanner.scan().expect("Expected scan to succeed");
            let expected = Loc {
                offset: 12620606,
                bit: 7,
            };
            assert_eq!(loc, expected);
        }

        #[test]
        fn block_fcn_returns_correct_bytes_with_no_shift() {
            let asm = vec![0x55];
            let r: &[u8] = &[0x55, 0x01, 0x02, 0x00, 0x00, 0x55, 0x03, 0x04, 0x00, 0x00];
            let mut scanner = Synchronizer::new(r, &asm, 2);

            // First block
            let loc = scanner.scan().expect("Expected scan 1 to succeed");
            let expected = Loc { offset: 2, bit: 0 };
            assert_eq!(loc, expected);
            let block = scanner.block().expect("Expected block 1 to succeed");
            assert_eq!(block, [0x01, 0x02]);

            // Second block
            let loc = scanner.scan().expect("Expected scan 2 to succeed");
            let expected = Loc { offset: 7, bit: 0 };
            assert_eq!(loc, expected);
            let block = scanner.block().expect("Expected block 2 to succeed");
            assert_eq!(block, [0x03, 0x04]);
        }

        #[test]
        fn block_fcn_returns_correct_bytes_when_shifted_1() {
            let asm = vec![0b01010101];
            let r: &[u8] = &[
                0b00101010, 0b10000000, 0b10000001, 0b00000000, 0b00000000, 0b00101010, 0b10000001,
                0b10000010, 0b00000000, 0b00000000, 0b00000000,
            ];
            let mut scanner = Synchronizer::new(&r[..], &asm, 2);

            // First block
            let loc = scanner.scan().expect("Expected scan 1 to succeed");
            let expected = Loc { offset: 2, bit: 7 };
            assert_eq!(loc, expected);
            let block = scanner.block().expect("Expected block 1 to succeed");
            assert_eq!(block, [0x01, 0x02]);

            // Second block
            let loc = scanner.scan().expect("Expected scan 2 to succeed");
            let expected = Loc { offset: 7, bit: 7 };
            assert_eq!(loc, expected);
            let block = scanner.block().expect("Expected block 2 to succeed");
            assert_eq!(block, [0x03, 0x04]);
        }
    }
}
