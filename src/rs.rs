pub use rs2::{correct_message, has_errors, RSState, PARITY_LEN};

/// Deinterleave an interleaved RS block (code block + check symbols).
///
/// # Panics
/// - If length of data is not a multiple of the interleave
///
/// Ref: 130.1-G-2, Section 5.3
pub fn deinterleave(data: &Vec<u8>, interleave: i32) -> Vec<[u8; 255]> {
    if data.len() % interleave as usize != 0 {
        panic!("data not a mulitpile of interleave({})", interleave);
    }
    let mut zult: Vec<[u8; 255]> = Vec::new();
    for _ in 0..interleave {
        zult.push([0u8; 255]);
    }
    for j in 0..data.len() as usize {
        zult[j % interleave as usize][j / interleave as usize] = data[j]
    }
    zult
}

pub trait ReedSolomon: Send {
    /// Correct an interleaved code block. This returns the code block data without the
    /// RS check symbols/bytes and a state that will be [`RSState::Uncorrectable`] if any
    /// single contained message is uncorrectable. If all messages are correctable the returned
    /// state will be [`RSState::Corrected`] with the total number of corrected bytes for
    /// all contained messages. If there are no errors return [`RSState::Ok`].
    ///
    /// The returned vector will be the original data without the RS parity bytes if
    /// uncorrectable or ok, otherwise it will be the corrected data without the RS parity
    /// bytes.
    ///
    /// # Panics
    /// - If the length of block is not a multiple of interleave
    fn correct_codeblock(&self, block: &[u8], interleave: i32) -> (Vec<u8>, RSState);
}

#[derive(Clone)]
pub struct DefaultReedSolomon;

impl ReedSolomon for DefaultReedSolomon {
    fn correct_codeblock(&self, block: &[u8], interleave: i32) -> (Vec<u8>, RSState) {
        let block: Vec<u8> = block.to_vec();
        if block.len() as i32 % interleave != 0 {
            panic!(
                "invalid block length for interleave {}: {}",
                interleave,
                block.len()
            );
        }

        // Length without the RS parity bytes. This is effectively the frame
        let data_len = block.len() - (interleave as usize * PARITY_LEN);

        let mut corrected = vec![0u8; block.len()];
        let mut num_corrected = 0;
        let messages = deinterleave(&block, interleave);
        for (idx, msg) in messages.iter().enumerate() {
            let zult = correct_message(msg);
            match zult.state {
                RSState::Uncorrectable(msg) => {
                    return (
                        block[..data_len].to_vec(),
                        RSState::Uncorrectable(format!(
                            "message {} is uncorrectable: {}",
                            idx, msg
                        )),
                    );
                }
                RSState::Corrected(num) => {
                    num_corrected += num;
                }
                _ => {}
            }
            let message = zult.message.expect("corrected rs message has no data");
            for j in 0..message.len() {
                corrected[idx + j * 4] = message[j];
            }
        }

        (
            corrected[..data_len].to_vec(),
            match num_corrected {
                0 => RSState::Ok, // no rs messages in block were corrected
                _ => RSState::Corrected(num_corrected),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RS message, no pn
    const FIXTURE_MSG: &[u8; 255] = &[
        0x67, 0xc4, 0x6b, 0xa7, 0x3e, 0xbe, 0x4c, 0x33, 0x6c, 0xb2, 0x23, 0x3a, 0x74, 0x06, 0x2b,
        0x18, 0xab, 0xb8, 0x09, 0xe6, 0x7d, 0xaf, 0x5d, 0xe5, 0xdf, 0x76, 0x25, 0x3f, 0xb9, 0x14,
        0xee, 0xec, 0xd1, 0xa3, 0x39, 0x5f, 0x38, 0x68, 0xf0, 0x26, 0xa6, 0x8a, 0xcb, 0x09, 0xaf,
        0x4e, 0xf8, 0x93, 0xf7, 0x45, 0x4b, 0x0d, 0xa9, 0xb8, 0x74, 0x0e, 0xf3, 0xc7, 0xed, 0x6e,
        0xa3, 0x0f, 0xf6, 0x79, 0x94, 0x16, 0xe2, 0x7f, 0xad, 0x91, 0x91, 0x04, 0xac, 0xa4, 0xae,
        0xb4, 0x51, 0x76, 0x2f, 0x62, 0x03, 0x5e, 0xa1, 0xe5, 0x5c, 0x45, 0xf8, 0x1f, 0x7a, 0x7b,
        0xe8, 0x35, 0xd8, 0xcc, 0x51, 0x0e, 0xae, 0x3a, 0x2a, 0x64, 0x1d, 0x03, 0x10, 0xcd, 0x18,
        0xe6, 0x7f, 0xef, 0xba, 0xd9, 0xe8, 0x98, 0x47, 0x82, 0x9c, 0xa1, 0x58, 0x47, 0x25, 0xdf,
        0x41, 0xd2, 0x01, 0x62, 0x3c, 0x24, 0x88, 0x90, 0xe9, 0xd7, 0x38, 0x1b, 0xa0, 0xa2, 0xb4,
        0x23, 0xea, 0x7e, 0x58, 0x0d, 0xf4, 0x61, 0x24, 0x14, 0xb0, 0x41, 0x90, 0x0c, 0xb7, 0xbb,
        0x5c, 0x59, 0x1b, 0xc6, 0x69, 0x24, 0x0f, 0xb6, 0x0e, 0x14, 0xa1, 0xb1, 0x8e, 0x48, 0x0f,
        0x17, 0x1d, 0xfb, 0x0f, 0x38, 0x42, 0xe3, 0x24, 0x58, 0xab, 0x82, 0xa8, 0xfd, 0xdf, 0xac,
        0x68, 0x93, 0x3d, 0x0d, 0x8f, 0x50, 0x52, 0x44, 0x6c, 0xba, 0xd3, 0x51, 0x99, 0x9c, 0x3e,
        0xad, 0xd5, 0xa8, 0xd7, 0x9d, 0xc7, 0x7f, 0x9f, 0xc9, 0x2a, 0xac, 0xe5, 0xc2, 0xcd, 0x9a,
        0x9b, 0xfa, 0x2d, 0x72, 0xab, 0x6b, 0xa4, 0x6b, 0x8b, 0x7d, 0xfa, 0x6c, 0x83, 0x63, 0x77,
        0x9f, 0x4e, 0x9a, 0x20, 0x35, 0xd2, 0x91, 0xce, 0xf4, 0x21, 0x1a, 0x97, 0x3c, 0x1a, 0x15,
        0x9d, 0xfc, 0x98, 0xba, 0x72, 0x1b, 0x9a, 0xa2, 0xe9, 0xc9, 0x46, 0x68, 0xce, 0xad, 0x27,
    ];

    #[test]
    fn test_deinterlace() {
        let dat: Vec<u8> = vec![0, 1, 2, 3, 0, 1, 2, 3];
        let blocks = deinterleave(&dat, 4);
        for i in 0..4 {
            assert_eq!(blocks[i][0], i as u8);
            assert_eq!(blocks[i][1], i as u8);
        }
    }

    #[test]
    fn test_correct_codeblock() {
        let interleave = 4;
        let mut block = vec![0u8; FIXTURE_MSG.len() * interleave];

        // Interleave the same message interleave number of times
        for j in 0..FIXTURE_MSG.len() {
            for i in 0..interleave {
                block[interleave * j + i] = FIXTURE_MSG[j];
            }
        }
        assert_eq!(block.len(), 1020); // sanity check

        // introduce an error by just adding one with wrap to a byte
        block[100] = block[100] + 1 % 255;
        //let block = block;

        let rs = DefaultReedSolomon {};
        let zult = rs.correct_codeblock(&block, interleave as i32);

        assert_eq!(
            zult.0.len(),
            892,
            "expect length 892 for I=4 header and frame data"
        );
        assert_eq!(zult.1, RSState::Corrected(1));
    }
}
