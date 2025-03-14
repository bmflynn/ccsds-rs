//! Pseudo-noise removal.
//!
//! This could definitely be made more general, however, since I have yet to come
//! across PN of any other format it specifically implements the PN documented in
//! the reference below.
//!
//! # References
//! 1. CCSDS TM Synchronization and Channel Coding; Section 10.
//!    - CCSDS 131.0-B-5
//!    - <https://public.ccsds.org/Pubs/131x0b5.pdf>
//!

/// Sequence used to derandomize. Generated using poly=0xa9, gen=0xff.
const SEQUENCE: [u8; 255] = [
    0xff, 0x48, 0x0e, 0xc0, 0x9a, 0x0d, 0x70, 0xbc, 0x8e, 0x2c, 0x93, 0xad, 0xa7, 0xb7, 0x46, 0xce,
    0x5a, 0x97, 0x7d, 0xcc, 0x32, 0xa2, 0xbf, 0x3e, 0x0a, 0x10, 0xf1, 0x88, 0x94, 0xcd, 0xea, 0xb1,
    0xfe, 0x90, 0x1d, 0x81, 0x34, 0x1a, 0xe1, 0x79, 0x1c, 0x59, 0x27, 0x5b, 0x4f, 0x6e, 0x8d, 0x9c,
    0xb5, 0x2e, 0xfb, 0x98, 0x65, 0x45, 0x7e, 0x7c, 0x14, 0x21, 0xe3, 0x11, 0x29, 0x9b, 0xd5, 0x63,
    0xfd, 0x20, 0x3b, 0x02, 0x68, 0x35, 0xc2, 0xf2, 0x38, 0xb2, 0x4e, 0xb6, 0x9e, 0xdd, 0x1b, 0x39,
    0x6a, 0x5d, 0xf7, 0x30, 0xca, 0x8a, 0xfc, 0xf8, 0x28, 0x43, 0xc6, 0x22, 0x53, 0x37, 0xaa, 0xc7,
    0xfa, 0x40, 0x76, 0x04, 0xd0, 0x6b, 0x85, 0xe4, 0x71, 0x64, 0x9d, 0x6d, 0x3d, 0xba, 0x36, 0x72,
    0xd4, 0xbb, 0xee, 0x61, 0x95, 0x15, 0xf9, 0xf0, 0x50, 0x87, 0x8c, 0x44, 0xa6, 0x6f, 0x55, 0x8f,
    0xf4, 0x80, 0xec, 0x09, 0xa0, 0xd7, 0x0b, 0xc8, 0xe2, 0xc9, 0x3a, 0xda, 0x7b, 0x74, 0x6c, 0xe5,
    0xa9, 0x77, 0xdc, 0xc3, 0x2a, 0x2b, 0xf3, 0xe0, 0xa1, 0x0f, 0x18, 0x89, 0x4c, 0xde, 0xab, 0x1f,
    0xe9, 0x01, 0xd8, 0x13, 0x41, 0xae, 0x17, 0x91, 0xc5, 0x92, 0x75, 0xb4, 0xf6, 0xe8, 0xd9, 0xcb,
    0x52, 0xef, 0xb9, 0x86, 0x54, 0x57, 0xe7, 0xc1, 0x42, 0x1e, 0x31, 0x12, 0x99, 0xbd, 0x56, 0x3f,
    0xd2, 0x03, 0xb0, 0x26, 0x83, 0x5c, 0x2f, 0x23, 0x8b, 0x24, 0xeb, 0x69, 0xed, 0xd1, 0xb3, 0x96,
    0xa5, 0xdf, 0x73, 0x0c, 0xa8, 0xaf, 0xcf, 0x82, 0x84, 0x3c, 0x62, 0x25, 0x33, 0x7a, 0xac, 0x7f,
    0xa4, 0x07, 0x60, 0x4d, 0x06, 0xb8, 0x5e, 0x47, 0x16, 0x49, 0xd6, 0xd3, 0xdb, 0xa3, 0x67, 0x2d,
    0x4b, 0xbe, 0xe6, 0x19, 0x51, 0x5f, 0x9f, 0x05, 0x08, 0x78, 0xc4, 0x4a, 0x66, 0xf5, 0x58,
];

fn flip_bits(b: u8) -> u8 {
    let mut x: u8 = 0;
    for (i, shft) in ([7, 5, 3, 1, -1, -3, -5, -7]).iter().enumerate() {
        if *shft > 0 {
            x |= (b & (1 << i)) << shft;
        } else {
            x |= (b & (1 << i)) >> -shft;
        }
    }

    x
}

/// Generates a 255 byte sequence of bytes containing the
/// pseudo random noise values to XOR againsts PSR encoded bytes. It is a
/// repeating pattern of 255 bits in the smallest whole number of bytes.
///
/// poly is the bit representation of the polonomial. For example:
/// x^8+x^6+x^4+x^1 will be 0xa9. gen is the generator, or initial seed value,
/// used to generate the sequence.
///
/// Used to generate `SEQUENCE`.
#[allow(dead_code)]
fn generate_pn_sequence(poly: u8, gen: u8) -> [u8; 255] {
    let mut table = [0u8; 255];
    table[0] = gen;

    for num in 1..255 {
        // logic works in a different order than byte ordering
        let mut gen = flip_bits(table[num - 1]);
        for _ in 0..8 {
            let mut b: u8 = 0;
            for pbit in 0..8 {
                if (poly >> pbit) & 1 == 1 {
                    // XOR with gen in the poly position
                    b ^= (gen >> pbit) & 1;
                }
            }
            // b goes on the front, then the high bits of gen
            gen = (b << 7) | (gen >> 1);
        }
        table[num] = flip_bits(gen);
    }

    table
}

fn _derandomize_loop(buf: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; buf.len()];
    for (idx, b) in buf.iter().enumerate() {
        out[idx] = b ^ SEQUENCE[idx % SEQUENCE.len()];
    }
    out
}

//fn _derandomize_ndarray(buf: &[u8]) -> Vec<u8> {
//    assert!(
//        buf.len() <= SEQUENCE.len(),
//        "data longer than the PN sequence: got {}, wanted < {}",
//        buf.len(),
//        SEQUENCE.len()
//    );
//    let arr = arr1(buf);
//    let seq = arr1(&SEQUENCE[..buf.len()]);
//
//    let zult = arr ^ seq;
//    zult.as_slice().unwrap().to_vec()
//}

/// An implementation of Pseudo-noise removal.
pub trait Derandomizer: Send + Sync {
    fn derandomize(&self, dat: &[u8]) -> Vec<u8>;
}

/// ``PNDecoder`` implementing standard CCSDS pseudo-noise derandomizon
/// (See [`TM Synchronization and Channel Coding`](https://public.ccsds.org/Pubs/131x0b5.pdf))
#[derive(Clone, Default)]
pub struct DefaultDerandomizer;

impl Derandomizer for DefaultDerandomizer {
    fn derandomize(&self, dat: &[u8]) -> Vec<u8> {
        _derandomize_loop(dat)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_filp_bits() {
        assert_eq!(flip_bits(0xaa), 0x55);
        assert_eq!(flip_bits(0), 0);
        assert_eq!(flip_bits(0xf0), 0xf);
    }

    #[test]
    fn test_generate_pn_sequence() {
        let sequence = [0xff, 0x48, 0xe, 0xc0];

        let table = generate_pn_sequence(0xa9, 0xff);
        for i in 0..sequence.len() {
            assert_eq!(
                sequence[i], table[i],
                "byte mismatch at idx {} got {}, expecte {}",
                i, table[i], sequence[i]
            );
        }
    }

    mod derandomize {
        use super::*;

        const DATA: [u8; 6] = [0x98, 0x18, 0x98, 0x82, 0x8c, 0x8d];
        const EXPECTED: [u8; 6] = [0x67, 0x50, 0x96, 0x42, 0x16, 0x80];

        #[test]
        fn test_loop() {
            let mut buf = [0u8; 6];
            buf.clone_from_slice(&DATA[..]);
            let zult = _derandomize_loop(&buf);

            for (i, (a, b)) in zult.iter().zip(EXPECTED.iter()).enumerate() {
                assert_eq!(*a, *b, "failed at index {i}");
            }
        }
    }
}
