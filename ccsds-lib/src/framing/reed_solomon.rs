use rs2::{correct_message, has_errors, RSState, N, PARITY_LEN};
use std::fmt::Display;

#[cfg(feature = "python")]
use pyo3::prelude::*;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// The possible integrity dispositions
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "python", pyclass(eq, eq_int, from_py_object))]
pub enum Integrity {
    /// Data did not require correction.
    Ok,
    /// Data had errors was successfully corrected.
    Corrected,
    /// Not correctable due to alg failure or too many errors
    Uncorrectable,
    /// Errors are present, but no correction was attempted.
    NotCorrected,
    /// No correction or detection was performed
    NotPerformed,
    /// Algorithm failed to run due to precondition, e.g., bad frame size
    Failed,
}

impl Display for Integrity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Integrity::Ok => write!(f, "ok"),
            Integrity::Corrected => write!(f, "corrected"),
            Integrity::Uncorrectable => write!(f, "uncorrectable"),
            Integrity::NotCorrected => write!(f, "notcorrected"),
            Integrity::NotPerformed => write!(f, "notperformed"),
            Integrity::Failed => write!(f, "failed"),
        }
    }
}

impl Integrity {
    /// Return `true` if [Self::Ok] or [Self::Corrected]. Any other value will return `false`.
    pub fn ok(&self) -> bool {
        matches!(self, Self::Ok | Self::Corrected)
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
    fn perform(&self, cadu_dat: &[u8]) -> Result<(Integrity, Vec<u8>)>;
}

/// Deinterleave an interleaved RS block (code block + check symbols).
///
/// ## Panics
/// - If length of data is not a multiple of the interleave
///
/// Ref: 130.1-G-2, Section 5.3
pub(super) fn deinterleave(data: &[u8], interleave: u8) -> Vec<[u8; 255]> {
    assert!(
        data.len().is_multiple_of(interleave as usize),
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
    correct: bool,
    detect: bool,
    interleave: u8,
    parity_len: usize,
    remove_parity: bool,
    virtual_fill: usize,
}

impl DefaultReedSolomon {
    /// Create a new instance with the given interleave that will by default use no virtual fill
    /// bytes and both detect and correct messages.
    ///
    /// # Arguments
    ///
    /// * `interleave` Interleaving depth. Each frame data to detect/correct must have a length
    ///   such that `length = interleave * 255 + vritual_fill`.
    pub fn new(interleave: u8) -> Self {
        Self {
            interleave,
            virtual_fill: 0,
            parity_len: PARITY_LEN,
            remove_parity: true,
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
    /// integrity is one of [Integrity::Ok] or [Integrity::Corrected].
    pub fn with_correction(mut self, enabled: bool) -> Self {
        self.correct = enabled;
        self
    }

    /// When `false` no RS algorithm will be performed.
    ///
    /// This may be useful for cases where you do not want to pay the penalty of running the
    /// algorithm but still want to remove the check symbols from the end of the code block.
    pub fn with_detection(mut self, enabled: bool) -> Self {
        self.detect = enabled;
        self
    }

    /// When true, remove the parity bytes from the end of the code block.
    pub fn with_remove_parity(mut self, enabled: bool) -> Self {
        self.remove_parity = enabled;
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
    /// If correction is disabled by passing `false` to [Self::with_correction] then the algorithm
    /// detects errors but does not perform the correction. If errors are detected the integrity
    /// will be [Integrity::NotCorrected]. This can save a significant amount of CPU.
    ///
    /// Check symbols are always removed before returning, if configured.
    fn perform(&self, cadu_dat: &[u8]) -> Result<(Integrity, Vec<u8>)> {
        if !DefaultReedSolomon::can_correct(cadu_dat, self.interleave, self.virtual_fill) {
            return Err(Error::IntegrityAlgorithm(format!(
                "codeblock len={} cannot be corrected by this algorithm with interleave={}",
                cadu_dat.len(),
                self.interleave,
            )));
        }

        // Basically, just removing parity
        if !self.correct && !self.detect {
            let cadu_dat = if self.remove_parity {
                self.remove_parity(cadu_dat)
            } else {
                cadu_dat
            };
            return Ok((Integrity::NotCorrected, cadu_dat.to_vec()));
        }

        // If using virtual fill, it gets added to the start of our CADU data
        let cadu_dat = if self.virtual_fill == 0 {
            cadu_dat
        } else {
            let zeros = &vec![0u8; self.virtual_fill];
            &[zeros, cadu_dat].concat()
        };

        // `cadu_dat` already includes any virtual fill here
        let mut corrected = vec![0u8; cadu_dat.len()];
        let mut num_corrected = 0;

        // Handle each individual RS message (there will be I of them)
        let messages = deinterleave(cadu_dat, self.interleave);
        for (idx, msg) in messages.iter().enumerate() {
            // If not doing correction, only detection, just bail at the first detected error
            if !self.correct {
                if has_errors(msg) {
                    // We have errors, no need to check other messages
                    let data = if self.remove_parity {
                        self.remove_parity(cadu_dat)
                    } else {
                        cadu_dat
                    };
                    return Ok((Integrity::NotCorrected, data[self.virtual_fill..].to_vec()));
                }
                // No errors in this message, so skip correction
                continue;
            }
            // Full algorithm, correct & detect
            let zult = correct_message(msg);
            match zult.state {
                RSState::Uncorrectable(_) => {
                    // Bail if there is any single uncorrectable message in this block
                    let data = if self.remove_parity {
                        self.remove_parity(cadu_dat)
                    } else {
                        cadu_dat
                    };
                    return Ok((Integrity::Uncorrectable, data[self.virtual_fill..].to_vec()));
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

        // Detect-only and no errors found, nothing was corrected so use the input as is
        if !self.correct {
            corrected = cadu_dat.to_vec();
        }

        let zult = if self.remove_parity {
            self.remove_parity(&corrected)
        } else {
            &corrected
        };

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

        // Check original data tests out OK
        let (status, block) = rs.perform(&cadu).unwrap();
        assert_eq!(
            status,
            Integrity::Ok,
            "expected source test data to not have errors, but it was not Ok"
        );
        assert_eq!(block.len(), expected_block_len);

        // Introduce an error by just adding one with wrap to a byte and make sure it's corrected
        cadu[100] += 1;
        let (status, block) = rs.perform(&cadu).unwrap();
        assert_eq!(
            status,
            Integrity::Corrected,
            "expected data to be corrected with introduced error, it was not"
        );
        assert_eq!(block.len(), expected_block_len);
    }

    /// Build a valid interleaved code block by repeating the fixture message.
    fn fixture_cadu(interleave: u8) -> Vec<u8> {
        let i = interleave as usize;
        let mut cadu = vec![0u8; FIXTURE_MSG.len() * i];
        for (j, b) in FIXTURE_MSG.iter().enumerate() {
            for k in 0..i {
                cadu[i * j + k] = *b;
            }
        }
        cadu
    }

    /// Expected output length when all parity is removed.
    fn data_len(interleave: u8) -> usize {
        (255 - PARITY_LEN) * interleave as usize
    }

    /// Expected output data. Interleaved fixture message without parity.
    fn expected_data(interleave: u8) -> Vec<u8> {
        let mut cadu = fixture_cadu(interleave);
        cadu.truncate(data_len(interleave));
        cadu
    }

    #[test]
    fn test_integrity_ok_removes_parity() {
        for interleave in [1u8, 4, 5] {
            let cadu = fixture_cadu(interleave);
            let rs = DefaultReedSolomon::new(interleave);
            let (status, data) = rs.perform(&cadu).unwrap();
            assert_eq!(status, Integrity::Ok, "interleave={interleave}");
            assert_eq!(data.len(), data_len(interleave), "interleave={interleave}");
            assert_eq!(data, expected_data(interleave), "interleave={interleave}");
        }
    }

    #[test]
    fn test_integrity_corrected_removes_parity_and_fixes_data() {
        for interleave in [1u8, 4, 5] {
            let mut cadu = fixture_cadu(interleave);
            cadu[10] ^= 0xff; // data byte error
            let last = cadu.len() - 1;
            cadu[last] ^= 0x01; // parity byte error
            let rs = DefaultReedSolomon::new(interleave);
            let (status, data) = rs.perform(&cadu).unwrap();
            assert_eq!(status, Integrity::Corrected, "interleave={interleave}");
            assert_eq!(data.len(), data_len(interleave), "interleave={interleave}");
            assert_eq!(data, expected_data(interleave), "interleave={interleave}");
        }
    }

    #[test]
    fn test_integrity_corrected_at_max_correctable_errors() {
        // 16 symbol errors in a single codeword is the most RS(255,223) can correct
        let interleave = 4u8;
        let mut cadu = fixture_cadu(interleave);
        for j in 0..16 {
            cadu[j * interleave as usize] ^= 0xa5;
        }
        let rs = DefaultReedSolomon::new(interleave);
        let (status, data) = rs.perform(&cadu).unwrap();
        assert_eq!(status, Integrity::Corrected);
        assert_eq!(data, expected_data(interleave));
    }

    #[test]
    fn test_integrity_uncorrectable_removes_parity() {
        let interleave = 4u8;
        let mut cadu = fixture_cadu(interleave);
        // 20 symbol errors in codeword 0, more than the 16 that can be corrected
        for j in 0..20 {
            cadu[j * interleave as usize] ^= 0xa5;
        }
        let rs = DefaultReedSolomon::new(interleave);
        let (status, data) = rs.perform(&cadu).unwrap();
        assert_eq!(status, Integrity::Uncorrectable);
        assert_eq!(data.len(), data_len(interleave));
    }

    #[test]
    fn test_uncorrectable_in_later_codeword() {
        // A single bad codeword anywhere in the block makes the whole block uncorrectable
        let interleave = 4u8;
        let mut cadu = fixture_cadu(interleave);
        for j in 0..20 {
            cadu[j * interleave as usize + 3] ^= 0xa5;
        }
        let rs = DefaultReedSolomon::new(interleave);
        let (status, data) = rs.perform(&cadu).unwrap();
        assert_eq!(status, Integrity::Uncorrectable);
        assert_eq!(data.len(), data_len(interleave));
    }

    #[test]
    fn test_no_correction_ok_removes_parity() {
        let interleave = 4u8;
        let cadu = fixture_cadu(interleave);
        let rs = DefaultReedSolomon::new(interleave).with_correction(false);
        let (status, data) = rs.perform(&cadu).unwrap();
        assert_eq!(status, Integrity::Ok);
        assert_eq!(data, expected_data(interleave));
    }

    #[test]
    fn test_no_correction_with_errors_removes_parity() {
        let interleave = 4u8;
        let mut cadu = fixture_cadu(interleave);
        cadu[10] ^= 0xff;
        let rs = DefaultReedSolomon::new(interleave).with_correction(false);
        let (status, data) = rs.perform(&cadu).unwrap();
        assert_eq!(status, Integrity::NotCorrected);
        assert_eq!(data.len(), data_len(interleave));
    }

    #[test]
    fn test_no_correction_detects_error_in_any_codeword() {
        // Error in the last codeword of the interleave must still be detected
        let interleave = 4u8;
        let mut cadu = fixture_cadu(interleave);
        cadu[3] ^= 0xff;
        let rs = DefaultReedSolomon::new(interleave).with_correction(false);
        let (status, data) = rs.perform(&cadu).unwrap();
        assert_eq!(status, Integrity::NotCorrected);
        assert_eq!(data.len(), data_len(interleave));
    }

    #[test]
    fn test_no_detection_removes_parity() {
        let interleave = 4u8;
        let mut cadu = fixture_cadu(interleave);
        cadu[10] ^= 0xff;
        let rs = DefaultReedSolomon::new(interleave)
            .with_correction(false)
            .with_detection(false);
        let (status, data) = rs.perform(&cadu).unwrap();
        assert_eq!(status, Integrity::NotCorrected);
        assert_eq!(data.len(), data_len(interleave));
        // Data is passed through unchanged
        assert_eq!(data, cadu[..data_len(interleave)]);
    }

    #[test]
    fn test_parity_kept_when_remove_parity_disabled() {
        let interleave = 4u8;
        let cadu = fixture_cadu(interleave);
        let rs = DefaultReedSolomon::new(interleave).with_remove_parity(false);
        let (status, data) = rs.perform(&cadu).unwrap();
        assert_eq!(status, Integrity::Ok);
        assert_eq!(data, cadu);
    }

    #[test]
    fn test_virtual_fill_removes_fill_and_parity() {
        // An all-zero block is a valid codeword, so it works with any amount of virtual fill.
        let interleave = 4u8;
        let fill = 100usize;
        let cadu = vec![0u8; 255 * interleave as usize - fill];
        let rs = DefaultReedSolomon::new(interleave).with_virtual_fill(fill);
        let (status, data) = rs.perform(&cadu).unwrap();
        assert_eq!(status, Integrity::Ok);
        assert_eq!(data.len(), data_len(interleave) - fill);
    }

    #[test]
    fn test_virtual_fill_corrected_removes_fill_and_parity() {
        let interleave = 4u8;
        let fill = 100usize;
        let mut cadu = vec![0u8; 255 * interleave as usize - fill];
        cadu[10] = 0x42;
        let rs = DefaultReedSolomon::new(interleave).with_virtual_fill(fill);
        let (status, data) = rs.perform(&cadu).unwrap();
        assert_eq!(status, Integrity::Corrected);
        assert_eq!(data.len(), data_len(interleave) - fill);
        assert!(data.iter().all(|b| *b == 0), "error was not corrected");
    }

    #[test]
    fn test_virtual_fill_uncorrectable_removes_fill_and_parity() {
        let interleave = 4u8;
        let fill = 100usize;
        let mut cadu = vec![0u8; 255 * interleave as usize - fill];
        for j in 0..20 {
            cadu[j * interleave as usize] = 0xa5;
        }
        let rs = DefaultReedSolomon::new(interleave).with_virtual_fill(fill);
        let (status, data) = rs.perform(&cadu).unwrap();
        assert_eq!(status, Integrity::Uncorrectable);
        assert_eq!(data.len(), data_len(interleave) - fill);
    }

    #[test]
    fn test_virtual_fill_no_correction_removes_fill_and_parity() {
        let interleave = 4u8;
        let fill = 100usize;
        let expected_len = data_len(interleave) - fill;
        let rs = DefaultReedSolomon::new(interleave)
            .with_virtual_fill(fill)
            .with_correction(false);

        let mut cadu = vec![0u8; 255 * interleave as usize - fill];
        let (status, data) = rs.perform(&cadu).unwrap();
        assert_eq!(status, Integrity::Ok);
        assert_eq!(data.len(), expected_len);

        cadu[10] = 0x42;
        let (status, data) = rs.perform(&cadu).unwrap();
        assert_eq!(status, Integrity::NotCorrected);
        assert_eq!(data.len(), expected_len);
        assert_eq!(data[10], 0x42, "data must not be modified");
    }

    #[test]
    fn test_no_correction_does_not_modify_data() {
        let interleave = 4u8;
        let cadu = fixture_cadu(interleave);
        let rs = DefaultReedSolomon::new(interleave).with_correction(false);
        let (_, data) = rs.perform(&cadu).unwrap();
        assert_eq!(data, cadu[..data_len(interleave)]);
    }

    #[test]
    fn test_bad_block_length_is_error() {
        let rs = DefaultReedSolomon::new(4);
        assert!(rs.perform(&fixture_cadu(4)[1..]).is_err());
        assert!(rs.perform(&fixture_cadu(5)).is_err());
    }

    #[test]
    fn test_integrity_ok() {
        assert!(Integrity::Ok.ok());
        assert!(Integrity::Corrected.ok());
        assert!(!Integrity::NotPerformed.ok());
        assert!(!Integrity::Uncorrectable.ok());
        assert!(!Integrity::NotCorrected.ok());
        assert!(!Integrity::Failed.ok());
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
