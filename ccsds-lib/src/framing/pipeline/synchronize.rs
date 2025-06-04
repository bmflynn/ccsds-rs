use std::io::Read;

#[cfg(feature = "python")]
use pyo3::{pyclass, pymethods};

use crate::framing::{synchronizer::Synchronizer, ASM};

use super::Cadu;

/// Options used for synchronization
#[derive(Clone, Debug)]
#[cfg_attr(feature = "python", pyclass(get_all, set_all))]
pub struct SyncOpts {
    pub asm: Vec<u8>,
    pub length: usize,
}

impl SyncOpts {
    /// Create a new set of sync options.
    ///
    /// # Arguments
    /// * `length` Length of data to return as a [Block](super), not including the length of the attached
    /// sync marker.
    pub fn new(length: usize) -> Self {
        SyncOpts {
            asm: ASM.to_vec(),
            length,
        }
    }
}

#[cfg_attr(feature = "python", pymethods)]
impl SyncOpts {
    #[cfg(feature = "python")]
    #[new]
    pub fn py_new(length: usize) -> Self {
        Self::new(length)
    }

    /// Attached sync marker indicating the start of a [Block].
    pub fn with_asm(&self, asm: &[u8]) -> Self {
        let mut slf = self.clone();
        slf.asm = asm.to_vec();
        slf
    }
}

/// Syncronize a bit stream to provide a byte-aligned iterator of [Block] data.
pub fn synchronize<R>(reader: R, opts: SyncOpts) -> impl Iterator<Item = Cadu>
where
    R: Read + Send,
{
    let sync = Synchronizer::new(reader, opts.length).with_asm(&opts.asm);

    sync.into_iter().filter_map(Result::ok)
}
