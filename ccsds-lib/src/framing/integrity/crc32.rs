use super::IntegrityAlgorithm;

pub struct DefaultCrc32 {
    /// Offset to the start of the crc checksum bytes
    offset: usize,
}

impl DefaultCrc32 {
    pub fn new(offset: usize) -> Self {
        Self { offset }
    }
}

impl IntegrityAlgorithm for DefaultCrc32 {
    fn remove_parity<'a>(&self, _cadu_dat: &'a [u8]) -> &'a [u8] {
        todo!();
    }
    fn perform(&self, _cadu_dat: &[u8]) -> super::Result<(super::Integrity, Vec<u8>)> {
        todo!();
    }
}
