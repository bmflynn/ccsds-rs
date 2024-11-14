pub use rs2::{correct_message, has_errors, RSState, N, PARITY_LEN};

#[derive(thiserror::Error, Debug)]
pub enum IntegrityError {
    #[error("input is not valid for this algorithm")]
    InvalidInput,
    #[error("input failed integrity check")]
    Failed,
}

/// Deinterleave an interleaved RS block (code block + check symbols).
///
/// ## Panics
/// - If length of data is not a multiple of the interleave
///
/// Ref: 130.1-G-2, Section 5.3
#[must_use]
pub fn deinterleave(data: &[u8], interleave: u8) -> Vec<[u8; 255]> {
    assert!(
        data.len() % interleave as usize == 0,
        "data length must be multiple of interleave"
    );
    let mut zult: Vec<[u8; 255]> = Vec::new();
    for _ in 0..interleave {
        zult.push([0u8; 255]);
    }
    for j in 0..data.len() {
        zult[j % interleave as usize][j / interleave as usize] = data[j];
    }
    zult
}

pub trait ReedSolomon: Send + Sync {
    /// Correct an interleaved code block, i.e., a codeblock that is `255 * interleave` in length.
    ///
    /// This returns the code block data without the RS check symbols/bytes and a state indicating
    /// the algorithm disposition.
    ///
    /// ## Errors
    /// This returns ``IntegrityError::InvalidInput`` if length of `block` is not `255 * interleave`.
    /// ``IntegrityError::Failed`` will be returned if correction is attempted but failed, likely
    /// because there were more errors found than could be corrected.
    ///
    /// ## Panics
    /// - If interleave is 0
    ///
    /// [can_correct]: Self::can_correct
    fn correct_codeblock(
        &self,
        block: &[u8],
        interleave: u8,
    ) -> Result<(Vec<u8>, RSState), IntegrityError>;
}

/// Implements the CCSDS documented Reed-Solomon (223/255) Forward Error Correct.
///
/// All blocks must must be a multiple of 255 bytes, otherwise ``Self::correct_codeblock`` will
/// return ``IntegrityError::InvalidInput``.
#[derive(Clone)]
pub struct DefaultReedSolomon;

impl DefaultReedSolomon {
    fn can_correct(block: &[u8], interleave: u8) -> bool {
        block.len() == N as usize * interleave as usize
    }

    fn strip_parity(block: &[u8], interleave: u8) -> Vec<u8> {
        // Length without the RS parity bytes. This is effectively the frame
        let data_len = block.len() - (interleave as usize * PARITY_LEN);
        block[..data_len].to_vec()
    }
}

impl ReedSolomon for DefaultReedSolomon {
    fn correct_codeblock(
        &self,
        block: &[u8],
        interleave: u8,
    ) -> Result<(Vec<u8>, RSState), IntegrityError> {
        assert!(interleave != 0, "interleave cannot be 0");

        if !DefaultReedSolomon::can_correct(block, interleave) {
            return Err(IntegrityError::InvalidInput);
        }

        let block: Vec<u8> = block.to_vec();
        let mut corrected = vec![0u8; block.len()];
        let mut num_corrected = 0;
        let messages = deinterleave(&block, interleave);
        for (idx, msg) in messages.iter().enumerate() {
            let zult = correct_message(msg);
            match zult.state {
                RSState::Uncorrectable(_) => {
                    return Err(IntegrityError::Failed);
                }
                RSState::Corrected(num) => {
                    num_corrected += num;
                }
                _ => {}
            }
            let message = zult.message.expect("corrected rs message has no data");
            for j in 0..message.len() {
                corrected[idx + j * interleave as usize] = message[j];
            }
        }

        let zult = Self::strip_parity(&corrected, interleave);
        let state = match num_corrected {
            0 => RSState::Ok, // no rs messages in block were corrected
            _ => RSState::Corrected(num_corrected),
        };

        Ok((zult, state))
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
        for (i, block) in blocks.iter().enumerate().take(4) {
            assert_eq!(block[0], u8::try_from(i).unwrap());
            assert_eq!(block[1], u8::try_from(i).unwrap());
        }
    }

    fn test_correct_codeblock(interleave: u8, blocksize: usize) {
        let mut block = vec![0u8; FIXTURE_MSG.len() * interleave as usize];

        // Interleave the same message interleave number of times
        for j in 0..FIXTURE_MSG.len() {
            for i in 0..interleave {
                block[interleave as usize * j + i as usize] = FIXTURE_MSG[j];
            }
        }
        assert_eq!(block.len(), blocksize); // sanity check

        // introduce an error by just adding one with wrap to a byte
        block[100] += 1;
        //let block = block;

        let rs = DefaultReedSolomon {};
        let zult = rs.correct_codeblock(&block, interleave);

        let (block, state) = zult.unwrap();
        let expected_block_len = if interleave == 4 { 892 } else { 1115 };
        assert_eq!(
            block.len(),
            expected_block_len,
            "expect length {expected_block_len} for I={interleave} header and frame data"
        );
        assert_eq!(state, RSState::Corrected(1));
    }

    #[test]
    fn test_correct_i4_1020_codeblock() {
        test_correct_codeblock(4, 1020);
    }

    #[test]
    fn test_correct_i5_1275_codeblock() {
        test_correct_codeblock(5, 1275);
    }
}
