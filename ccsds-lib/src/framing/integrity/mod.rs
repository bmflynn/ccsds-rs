mod crc32;
mod reed_solomon;

pub use crate::prelude::*;
pub use crc32::*;
pub use reed_solomon::*;

use super::VCDUHeader;

#[derive(Clone, Debug, PartialEq)]
pub enum Integrity {
    NoErrors,
    HasErrors,
    /// Data did not require correction.
    Ok,
    /// Data was successfully corrected.
    Corrected,
    Uncorrectable,
    /// The algorithm choose to skip performing integrity checks
    Skipped,
}

pub trait IntegrityAlgorithm: Send + Sync {
    /// Perform this integrity check.
    ///
    /// `cadu_dat` must already be derandomized and be of expected lenght for this algorithm. This
    /// algorithm may also choose to skip performance of the algorithm, e.g., for VCID fill frames.
    ///
    /// The algorithm will remove any parity bytes such that the returned data is just the frame
    /// bytes.
    fn perform(&self, header: &VCDUHeader, cadu_dat: &[u8]) -> Result<(Integrity, Vec<u8>)>;
}
