use std::convert::TryInto;
use std::error::Error;
use std::io;

use crate::error::DecodeError;
use crate::timecode::{CDSTimecode, EOSCUCTimecode, HasTimecode};
use crate::PrimaryHeader;

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
/// use std::io;
/// use ccsds::timecode::{
///     CDSTimecode,
///     HasTimecode
/// };
/// use ccsds::packet::Packet;
///
/// let dat: &[u8] = &[
///     // primary header bytes
///     0xd, 0x59, 0xd2, 0xab, 0x0, 0x7,
///     // CDS timecode bytes in secondary header
///     0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
///     // minimum 1 byte of user data
///     0xff
/// ];
/// let mut r = io::BufReader::new(dat);
/// let packet = Packet::read(&mut r).unwrap();
/// let tc: CDSTimecode = packet.timecode().unwrap();
/// ```
pub struct Packet {
    /// All packets have a primary header
    pub header: PrimaryHeader,
    /// User data, which may include a secondary header
    pub data: Vec<u8>,
}

impl Packet {
    pub fn read(r: &mut impl io::Read) -> Result<Packet, Box<dyn Error>> {
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

impl HasTimecode<CDSTimecode> for Packet {
    /// Reads a timecode from packet user data.
    fn timecode(&self) -> Result<CDSTimecode, DecodeError> {
        if !self.header.has_secondary_header {
            return Err(DecodeError::Other(String::from(
                "cannot get timecode without secondary header",
            )));
        }
        if self.data.len() < CDSTimecode::SIZE {
            return Err(DecodeError::Other(format!(
                "expected {} bytes for CDSTimecode, got {}", CDSTimecode::SIZE, self.data.len())));
        }

        // convert 8 bytes of time data into u64
        let (bytes, _) = self.data.split_at(CDSTimecode::SIZE);

        CDSTimecode::new(bytes)
    }
}

impl HasTimecode<EOSCUCTimecode> for Packet {
    /// Reads a EOSCUCTimecode from packet data zone.
    fn timecode(&self) -> Result<EOSCUCTimecode, DecodeError> {
        if !self.header.has_secondary_header {
            return Err(DecodeError::Other(String::from(
                "cannot get timecode without secondary header",
            )));
        }
        if self.data.len() < EOSCUCTimecode::SIZE {
            return Err(DecodeError::Other(format!(
                "expected {} bytes for EOSCUCTimecode, got {}", EOSCUCTimecode::SIZE, self.data.len())));
        }

        // There is an extra byte of data before timecode
        let (bytes, _) = self.data[1..].split_at(EOSCUCTimecode::SIZE);
        // we've already ensured we have enough bytes, so this won't panic

        EOSCUCTimecode::new(bytes.try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn test_get_cdstimecode() {
        let dat: &[u8] = &[
            // Primary/secondary header and a single byte of user data
            0xd, 0x59, 0xd2, 0xab, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];
        let mut r = io::BufReader::new(dat);
        let packet = Packet::read(&mut r).unwrap();

        // Just make sure we can get the timestamp
        let tc: CDSTimecode = packet.timecode().unwrap();
        assert_eq!(tc.days, 21184);
        assert_eq!(tc.millis, 167);
        assert_eq!(tc.micros, 219);
    }
}
