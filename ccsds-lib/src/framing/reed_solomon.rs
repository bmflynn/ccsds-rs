use rs2::{correct_message, has_errors, RSState, N, PARITY_LEN};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{framing::VCDUHeader, Error, Result};

/// The possible integrity dispositions
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Integrity {
    /// Data did not require correction.
    Ok,
    /// Data was successfully corrected.
    Corrected,
    /// Not correctable due to alg failure or too many errors
    Uncorrectable,
    /// Errors are present, but no correction was attempted.
    NotCorrected,
    /// The algorithm choose to skip performing integrity checks
    Skipped,
    /// Algorithm failed to run due to precondition, e.g., bad frame size
    Failed,
}

impl Integrity {
    /// Return `true` if [Self::Ok] or [Self::Corrected]. Any other value will return `false`.
    pub fn ok(&self) -> bool {
        match self {
            Self::Ok | Self::Corrected => true,
            _ => false,
        }
    }
}

pub trait ReedSolomon: Send + Sync {
    /// Perform this integrity check.
    ///
    /// `cadu_dat` must already be derandomized and be of expected length for this algorithm. This
    /// algorithm may also choose to skip performance of the algorithm, e.g., for VCID fill frames.
    ///
    /// The algorithm will remove any parity bytes such that the returned data is just the frame
    /// bytes.
    fn perform(&self, header: &VCDUHeader, cadu_dat: &[u8]) -> Result<(Integrity, Vec<u8>)>;
}

/// Deinterleave an interleaved RS block (code block + check symbols).
///
/// ## Panics
/// - If length of data is not a multiple of the interleave
///
/// Ref: 130.1-G-2, Section 5.3
fn deinterleave(data: &[u8], interleave: u8) -> Vec<[u8; 255]> {
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

/// CCSDS documented Reed-Solomon (223/255) Forward Error Correction.
///
/// # References
/// * [TM Synchronization and Channel Coding](https://ccsds.org/Pubs/131x0b5.pdf), Section 4
#[derive(Clone, Debug)]
pub struct DefaultReedSolomon {
    interleave: u8,
    virtual_fill: usize,
    parity_len: usize,
    detect: bool,
    correct: bool,
}

impl DefaultReedSolomon {
    /// Create a new instance with the given interleave that will by default use no virtual fill
    /// bytes and both detect and correct messages.
    ///
    /// # Arguments
    ///
    /// * `interleave` Interleaving depth. Each frame data to detect/correct must have a length
    /// such that `length = interleave * 255 + vritual_fill`.
    pub fn new(interleave: u8) -> Self {
        Self {
            interleave,
            virtual_fill: 0,
            parity_len: PARITY_LEN,
            detect: true,
            correct: true,
        }
    }

    /// Set the number of virtual fill bytes that should be added before performing the RS
    /// algorithm on each codeblock.
    ///
    /// Virtual fill are zero-bytes added to the start of the algorithm input data to pad it out to
    /// be the correct length for the interleave and algorithm requirement, e.g., `I=4: I * 255 =
    /// 1020`.
    pub fn with_virtual_fill(mut self, num: usize) -> Self {
        self.virtual_fill = num;
        self
    }

    /// If `false` no data correction will be performed. Any data that has detected errors will get
    /// the integrity value [Integrity::NotCorrected] and check symbols will _NOT_ be removed.
    ///
    /// If `true` (default), correction will be attempted and check symbols will be removed if the
    /// integrity is one of [Integrity::Ok], [Integrity::Corrected], or [Integrity::Skipped].
    pub fn with_correction(mut self, enabled: bool) -> Self {
        self.correct = enabled;
        self
    }

    /// When `false` no RS algorithm will be performed.
    ///
    /// Check symbols will be removed.
    ///
    /// This may be used when the data is known to be good to avoid the computation penalty of
    /// running the algorithm.
    pub fn with_detection(mut self, enabled: bool) -> Self {
        self.detect = enabled;
        self
    }

    fn can_correct(block: &[u8], interleave: u8, virtual_fill: usize) -> bool {
        block.len() + virtual_fill == N as usize * interleave as usize
    }

    fn remove_parity<'a>(&self, cadu_dat: &'a [u8]) -> &'a [u8] {
        let parity_len = self.interleave as usize * self.parity_len;
        &cadu_dat[..cadu_dat.len() - parity_len]
    }
}

impl ReedSolomon for DefaultReedSolomon {
    /// Performs the algorithm.
    ///
    /// Check symbols are removed if the integrity is [Integrity::Ok], [Integrity::Corrected],
    /// [Integrity::Skipped]. They are not removed for [Integrity::NotCorrected], [Integrity::Failed],
    /// or [Integrity::Uncorrectable].
    ///
    /// If correction is disabled by passing `false` to [Self::with_correction] then the algorithm
    /// detects errors but does not perform the correction. If errors are detected the integrity
    /// will be [Integrity::NotCorrected]. This can save a significant amount of CPU.
    ///
    /// In the case detection is disabled by passing `false` to [Self::with_detection] then neither
    /// the detection or correction are performed, however, check symbols are still removed.
    fn perform(&self, header: &VCDUHeader, cadu_dat: &[u8]) -> Result<(Integrity, Vec<u8>)> {
        if !DefaultReedSolomon::can_correct(cadu_dat, self.interleave, self.virtual_fill) {
            return Err(Error::IntegrityAlgorithm(format!(
                "codeblock len={} cannot be corrected by this algorithm with interleave={}",
                cadu_dat.len(),
                self.interleave,
            )));
        }

        if header.vcid == VCDUHeader::FILL || !self.detect {
            return Ok((Integrity::Skipped, self.remove_parity(cadu_dat).to_vec()));
        }

        let block: Vec<u8> = cadu_dat.to_vec();
        let mut corrected = vec![0u8; block.len() + self.virtual_fill];
        let mut num_corrected = 0;

        // If using virtual fill, it gets added to the start of our CADU data
        let cadu_dat = if self.virtual_fill == 0 {
            cadu_dat
        } else {
            let zeros = &vec![0u8; self.virtual_fill];
            &[zeros, cadu_dat].concat()
        };

        let messages = deinterleave(cadu_dat, self.interleave);
        for (idx, msg) in messages.iter().enumerate() {
            if !self.correct && has_errors(msg) {
                return Ok((Integrity::NotCorrected, cadu_dat.to_vec()));
            }
            let zult = correct_message(msg);
            match zult.state {
                RSState::Uncorrectable(_) => {
                    // Bail if there is any single uncorrectable message in this block
                    let cadu_data = self.remove_parity(cadu_dat);
                    return Ok((Integrity::Uncorrectable, cadu_data.to_vec()));
                }
                RSState::Corrected(num) => {
                    num_corrected += num;
                }
                _ => {}
            }
            let message = zult.message.expect("corrected rs message has no data");
            for j in 0..message.len() {
                corrected[idx + j * self.interleave as usize] = message[j];
            }
        }

        // The resulting buffer does not include the parity bytes
        let zult = self.remove_parity(&corrected);
        // Remove any added virtual fill zeros
        let zult = &zult[self.virtual_fill..];
        match num_corrected {
            0 => Ok((Integrity::Ok, zult.to_vec())),
            _ => Ok((Integrity::Corrected, zult.to_vec())),
        }
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
        let mut cadu = vec![0u8; FIXTURE_MSG.len() * interleave as usize];

        // Interleave the same message interleave number of times
        for j in 0..FIXTURE_MSG.len() {
            for i in 0..interleave {
                cadu[interleave as usize * j + i as usize] = FIXTURE_MSG[j];
            }
        }
        assert_eq!(cadu.len(), blocksize); // sanity check

        let rs = DefaultReedSolomon::new(interleave);
        let expected_block_len = if interleave == 4 { 892 } else { 1115 };
        let hdr = VCDUHeader::decode(&cadu).unwrap();

        // Check original data tests out OK
        let (status, block) = rs.perform(&hdr, &cadu).unwrap();
        assert_eq!(
            status,
            Integrity::Ok,
            "expected source test data to not have errors, but it was not Ok"
        );
        assert_eq!(block.len(), expected_block_len);

        // Introduce an error by just adding one with wrap to a byte and make sure it's corrected
        cadu[100] += 1;
        let (status, block) = rs.perform(&hdr, &cadu).unwrap();
        assert_eq!(
            status,
            Integrity::Corrected,
            "expected data to be corrected with introduced error, it was not"
        );
        assert_eq!(block.len(), expected_block_len);
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
