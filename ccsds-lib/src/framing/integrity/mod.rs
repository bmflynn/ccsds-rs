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
    fn perform(&self, cadu_dat: &[u8]) -> Result<(Integrity, Vec<u8>)>;
}
