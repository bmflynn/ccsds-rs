// pub struct DefaultCrc32 {
//     /// Offset to the start of the crc checksum bytes
//     offset: usize,
//     size: usize,
//     alg: crc::Crc<u32>,
// }
//
// impl DefaultCrc32 {
//     const CRC_SIZE: usize = 4;
//     pub fn new(offset: usize) -> Self {
//         Self {
//             offset,
//             size: offset + Self::CRC_SIZE,
//             alg: crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC),
//         }
//     }
// }
//
// //  impl ReedSolomon for DefaultCrc32 {
//      fn perform(&self, header: &VCDUHeader, cadu_dat: &[u8]) -> Result<(Integrity, Vec<u8>)> {
//          if cadu_dat.len() < self.size {
//              return Err(Error::NotEnoughData {
//                  got: cadu_dat.len(),
//                  wanted: self.size,
//              });
//          }
//          if header.vcid == VCDUHeader::FILL {
//              return Ok((Integrity::Skipped, cadu_dat.to_vec()));
//          }
//          let dat = &cadu_dat[self.offset..self.offset + Self::CRC_SIZE];
//          let expected = u32::from_be_bytes([dat[0], dat[1], dat[2], dat[3]]);
//          if expected != self.alg.checksum(cadu_dat) {
//              return Ok((Integrity::NoErrors, cadu_dat.to_vec()));
//          }
//          Ok((Integrity::HasErrors, cadu_dat.to_vec()))
//      }
//  }
