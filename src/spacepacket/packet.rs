use std::error::Error;
use std::{convert::TryInto, io::Read};

use super::{PrimaryHeader};

/// Packet represents a single CCSDS space packet and its associated data.
///
/// This packet contains the primary header data as well as the user data,
/// which may or may not container a secondary header. See the header's
/// `has_secondary_header` flag.
///
/// # Example
/// Create a packet from the minumum number of bytes. This example includes
/// bytes for a `CDSTimecode` in the data zone.
/// ```
/// use ccsds::spacepacket::{
///     CDSTimecode,
///     parse_cds_timecode,
///     Packet,
///     PrimaryHeader,
/// };
///
/// let dat: &[u8] = &[
///     // primary header bytes
///     0xd, 0x59, 0xd2, 0xab, 0x0, 0x7,
///     // CDS timecode bytes in secondary header
///     0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
///     // minimum 1 byte of user data
///     0xff
/// ];
/// let mut r = std::io::BufReader::new(dat);
/// let packet = Packet::read(&mut r).unwrap();
/// let tc = parse_cds_timecode(&packet.data[PrimaryHeader::SIZE..]);
/// ```
pub struct Packet {
    /// All packets have a primary header
    pub header: PrimaryHeader,
    /// User data, which may include a secondary header
    pub data: Vec<u8>,
}

impl Packet {
    pub fn read(r: &mut dyn Read) -> Result<Packet, Box<dyn Error>> {
        let ph = PrimaryHeader::read(r)?;

        // read the user data, shouldn't panic since unpacking worked
        let mut buf = vec![0u8; (ph.len_minus1 + 1).try_into().unwrap()];

        r.read_exact(&mut buf)?;

        Ok(Packet {
            header: ph,
            data: buf,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    #[test]
    fn test_read_packet() {
        let dat: [u8; 15] = [
            // Primary/secondary header and a single byte of user data
            0xd, 0x59, 0xd2, 0xab, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];
        let x = &dat[..];
        let mut r = io::BufReader::new(x);
        let packet = Packet::read(&mut r).unwrap();

        assert_eq!(packet.header.version, 0);
    }
}
