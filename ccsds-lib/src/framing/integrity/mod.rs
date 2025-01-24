mod crc32;
mod reed_solomon;

pub use crate::prelude::*;
pub use crc32::*;
pub use reed_solomon::*;

#[derive(Clone, Debug, PartialEq)]
pub enum Integrity {
    NoErrors,
    HasErrors,
    /// Data did not require correction.
    Ok,
    /// Data was successfully corrected.
    Corrected,
    Uncorrectable,
}

pub trait IntegrityAlgorithm: Send + Sync {
    /// Remove parity bytes from the CADU data.
    ///
    /// This does not imply that this integrity check was performed.
    fn remove_parity<'a>(&self, cadu_dat: &'a [u8]) -> &'a [u8];

    /// Perform this integrity check. The returned data will have any parity bytes removed.
    fn perform(&self, cadu_dat: &[u8]) -> Result<(Integrity, Vec<u8>)>;
}
