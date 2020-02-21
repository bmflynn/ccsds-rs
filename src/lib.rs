/// ccsds packet decoding library.
///
/// References:
/// * CCSDS Space Packet Protocol 133.0-B-1
///     - https://public.ccsds.org/Pubs/133x0b1c2.pdf
///
extern crate packed_struct;
#[macro_use]
extern crate packed_struct_codegen;

pub mod error;

use chrono::{DateTime, Duration, TimeZone, Utc};
use packed_struct::prelude::*;

use std::convert::TryInto;
use std::io; use std::vec;

use error::DecodeError;

/// CCSDS Primary Header
#[derive(PackedStruct, Debug)]
#[packed_struct(endian = "msb", bit_numbering = "msb0")]
pub struct PrimaryHeader {
    #[packed_field(bits = "0:2")]
    version: Integer<u8, packed_bits::Bits3>,
    #[packed_field(size_bits = "1")]
    is_test: bool,
    #[packed_field(size_bits = "1")]
    has_secondary_header: bool,
    #[packed_field(size_bits = "11")]
    apid: Integer<u16, packed_bits::Bits11>,
    #[packed_field(size_bits = "2")]
    sequence_flags: Integer<u8, packed_bits::Bits2>,
    #[packed_field(size_bits = "14")]
    sequence_id: Integer<u16, packed_bits::Bits14>,
    #[packed_field(size_bits = "16")]
    len_minus1: u16,
}

impl PrimaryHeader {
    // packed_bits is not static
    // const SIZE: usize = PrimaryHeader::packed_bits();
    const SIZE: usize = 6;
}

pub fn read_header(r: &mut impl io::Read) -> Result<PrimaryHeader, DecodeError> {
    let mut buf = [0u8; PrimaryHeader::SIZE];
    r.read_exact(&mut buf)?;
    Ok(PrimaryHeader::unpack(&buf)?)
}

/// CCSDS Day-Segmented Timecode
#[derive(PackedStruct, Debug)]
#[packed_struct(endian = "msb", bit_numbering = "msb0")]
pub struct CDSTimecode {
    #[packed_field(size_bits = "16")]
    pub days: u16,
    #[packed_field(size_bits = "32")]
    pub millis: u32,
    #[packed_field(size_bits = "16")]
    pub micros: u16,
}

impl CDSTimecode {
    // Seconds between Unix epoch(1970) and CDS epoch(1958)
    const EPOCH_DELTA: i64 = 378691200;
    const SIZE: usize = 8;

    pub fn timestamp(&self) -> DateTime<Utc> {
        Utc.timestamp_nanos(
            ((self.days as u64) * 86400 * (1e9 as u64)
                + (self.millis as u64) * (1e6 as u64)
                + (self.micros as u64) * (1e3 as u64)) as i64,
        ) - Duration::seconds(CDSTimecode::EPOCH_DELTA)
    }
}

pub struct Packet {
    pub header: PrimaryHeader,
    // Secondary header and any user data
    pub data: Vec<u8>,
}

impl Packet {
    pub fn timecode(&self) -> Result<CDSTimecode, io::Error> {
        if self.data.len() < CDSTimecode::SIZE {
            return Err(io::Error::new(io::ErrorKind::Other, "not enough bytes"));
        }

        // convert 8 bytes of time data into u64
        let (bytes, _) = self.data.split_at(std::mem::size_of::<u64>());
        let x = u64::from_be_bytes(match bytes.try_into() {
            Ok(arr) => arr,
            Err(err) => return Err(io::Error::new(io::ErrorKind::Other, err))
        });

        Ok(CDSTimecode {
            days: (x >> 48 & 0xffff) as u16,
            millis: (x >> 16 & 0xffffffff) as u32,
            micros: (x & 0xffff) as u16,
        })
    }
}

pub fn read_packet(r: &mut impl io::Read) -> Result<Packet, DecodeError> {
    let ph = read_header(r)?;

    // read the user data
    let size = (ph.len_minus1 + 1).try_into()?;
    let mut buf = vec![0u8; size];
    r.read_exact(&mut buf)?;

    Ok(Packet {
        header: ph,
        data: buf,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn test_decode_header() {
        let dat: [u8; 6] = [
            // bytes from a SNPP CrIS packet
            0xd, 0x59, 0xd2, 0xab, 0xa, 0x8f,
        ];
        let x = &dat[0..];
        let mut r = BufReader::new(x);
        let ph = read_header(&mut r).unwrap();

        assert_eq!(ph.version.to_primitive(), 0);
        assert_eq!(ph.is_test, false);
        assert_eq!(ph.has_secondary_header, true);
        assert_eq!(ph.apid.to_primitive(), 1369);
        assert_eq!(ph.sequence_flags.to_primitive(), 3);
        assert_eq!(ph.sequence_id.to_primitive(), 4779);
        assert_eq!(ph.len_minus1, 2703);
    }

    #[test]
    fn test_read_packet() {
        let dat: [u8; 15] = [
            // Primary/secondary header and a single byte of user data
            0xd, 0x59, 0xd2, 0xab, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];
        let x = &dat[..];
        let mut r = BufReader::new(x);
        let packet = read_packet(&mut r).unwrap();

        assert_eq!(packet.header.version.to_primitive(), 0);
        assert_eq!(packet.header.is_test, false);
        assert_eq!(packet.header.has_secondary_header, true);
        assert_eq!(packet.header.apid.to_primitive(), 1369);
        assert_eq!(packet.header.sequence_flags.to_primitive(), 3);
        assert_eq!(packet.header.sequence_id.to_primitive(), 4779);
        assert_eq!(packet.header.len_minus1, 8);
        assert_eq!(packet.data[packet.data.len()-1], 0xff);

        // Just make sure we can get the timestamp
        packet.timecode().unwrap();
    }

    #[test]
    fn test_cds_timecode() {
        let cds = CDSTimecode{
            days: 21184,
            millis: 167,
            micros: 219,
        };
        let ts = cds.timestamp();
        assert_eq!(ts.timestamp_millis(), 1451606400167);
    }
}
