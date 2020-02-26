/// ccsds packet decoding library.
///
/// References:
/// * CCSDS Space Packet Protocol 133.0-B-1
///     - https://public.ccsds.org/Pubs/133x0b1c2.pdf
///

pub mod error;
pub mod packet;
pub mod stream;
pub mod timecode;

use std::io;

/// CCSDS Primary Header
///
/// The primary header format is common to all CCSDS space packets.
///
#[derive(Debug, Copy, Clone)]
pub struct PrimaryHeader {
    pub version: u8,
    pub type_flag: u8,
    pub has_secondary_header: bool,
    pub apid: u16,
    pub sequence_flags: u8,
    pub sequence_id: u16,
    pub len_minus1: u16,
}

impl PrimaryHeader {
    /// Size of a PrimaryHeader
    const SIZE: usize = 6;

    /// Reads a packet from `r`
    ///
    /// # Errors
    ///
    /// * `DecodeError` If the packet cannot be read from `r` or unpacked from
    ///   the resulting bytes.
    ///
    pub fn read<T: io::Read>(r: &mut T) -> Result<PrimaryHeader, Box<dyn std::error::Error>> {
        let mut buf = [0u8; Self::SIZE];
        r.read_exact(&mut buf)?;

        let d1 = u16::from_be_bytes([buf[0], buf[1]]);
        let d2 = u16::from_be_bytes([buf[2], buf[3]]);
        let d3 = u16::from_be_bytes([buf[4], buf[5]]);

        Ok(PrimaryHeader{
            version: (d1 >> 13 & 0x7) as u8,
            type_flag: (d1 >> 12 & 0x1) as u8,
            has_secondary_header: (d1 >> 11 & 0x1) == 1,
            apid: (d1 & 0x7ff) as u16,
            sequence_flags: (d2 >> 14 & 0x3) as u8,
            sequence_id: (d2 & 0x3fff) as u16,
            len_minus1: d3,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn test_read_header() {
        let dat: [u8; 6] = [
            // bytes from a SNPP CrIS packet
            0xd, 0x59, 0xd2, 0xab, 0xa, 0x8f,
        ];
        let x = &dat[0..];
        let mut r = BufReader::new(x);
        let ph = PrimaryHeader::read(&mut r).unwrap();

        assert_eq!(ph.version, 0);
        assert_eq!(ph.type_flag, 0);
        assert_eq!(ph.has_secondary_header, true);
        assert_eq!(ph.apid, 1369);
        assert_eq!(ph.sequence_flags, 3);
        assert_eq!(ph.sequence_id, 4779);
        assert_eq!(ph.len_minus1, 2703);
    }
}
