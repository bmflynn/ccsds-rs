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

///// Checks the integrity of CADU payload data.
//pub trait Checker: Send + Sync {
//    /// Perform integrity checks on `cadu_dat` returning `true` if errors are present
//    ///
//    /// `dat` must include all CADU bytes except the ASM.
//    ///
//    ///
//    ///
//    /// # Errors
//    /// [Error] if the algorithm was unable to perform the check
//    fn check(&self, cadu_dat: &[u8]) -> Result<Integrity>;
//}
//
//pub trait Corrector: Send + Sync {
//    /// Perform integrity correction on `cadu_dat`.
//    ///
//    /// `dat` must include all CADU bytes except the ASM.
//    ///
//    /// When data is successfully corrected the returned data will not include any parity bytes,
//    /// otherwise the original data will be returned.
//    ///
//    /// # Errors
//    /// [Error] if the algorithm was unable to perform data correction.
//    fn correct(&self, cadu_dat: &[u8]) -> Result<Integrity>;
//}
