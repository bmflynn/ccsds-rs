extern crate packed_struct;
#[macro_use]
extern crate packed_struct_codegen;

use chrono::{DateTime, Duration, TimeZone, Utc};
use packed_struct::prelude::*;

use std::io;

// XXX: Seems like it should be possible to get the size from the struct
pub const PRIMARY_HDR_SIZE: usize = 6;
pub const CDS_TIMECODE_SIZE: usize = 8;

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

pub fn read_header(r: &mut impl io::Read) -> Result<PrimaryHeader, io::Error> {
    let mut buf: [u8; PRIMARY_HDR_SIZE] = [0; PRIMARY_HDR_SIZE];
    r.read_exact(&mut buf)?;
    match PrimaryHeader::unpack(&buf) {
        Ok(h) => Ok(h),
        Err(err) => Err(io::Error::new(io::ErrorKind::Other, err)),
    }
}

#[derive(PackedStruct, Debug)]
#[packed_struct(endian = "msb", bit_numbering = "msb0")]
pub struct CDSTimecode {
    #[packed_field(size_bits = "16")]
    days: u16,
    #[packed_field(size_bits = "32")]
    millis: u32,
    #[packed_field(size_bits = "16")]
    micros: u16,
}

impl CDSTimecode {
    // Seconds between Unix epoch and CDS epoch
    const EPOCH_DELTA: i64 = 378691200;

    pub fn timestamp(&self) -> DateTime<Utc> {
        Utc.timestamp_nanos(
            ((self.days as u64) * 86400 * (1e9 as u64)
                + (self.millis as u64) * (1e6 as u64)
                + (self.micros as u64) * (1e3 as u64)) as i64,
        ) - Duration::seconds(CDSTimecode::EPOCH_DELTA)
    }
}

pub fn decode_cds(dat: u64) -> CDSTimecode {
    CDSTimecode {
        days: (dat >> 48 & 0xffff) as u16,
        millis: (dat >> 16 & 0xffffffff) as u32,
        micros: (dat & 0xffff) as u16,
    }
}

#[derive(PackedStruct, Debug)]
#[packed_struct(endian = "msb", bit_numbering = "msb0")]
pub struct JPSSSecondaryHeader {
    #[packed_field(element_size_bytes = "8")]
    timecode: CDSTimecode,
    #[packed_field(size_bits = "8")]
    num_packets: u8,
    #[packed_field(size_bits = "8")]
    _spare: u8,
}

const JPSS_SECONDARY_HDR_SIZE: usize = 10;

pub fn read_jpss_secondary_header(r: &mut impl io::Read) -> Result<JPSSSecondaryHeader, io::Error> {
    let mut buf: [u8; JPSS_SECONDARY_HDR_SIZE] = [0; JPSS_SECONDARY_HDR_SIZE];
    r.read_exact(&mut buf)?;
    match JPSSSecondaryHeader::unpack(&buf) {
        Ok(h) => Ok(h),
        Err(err) => Err(io::Error::new(io::ErrorKind::Other, err)),
    }
}

pub struct Packet {
    header: PrimaryHeader,
    secondary_header: Option<JPSSSecondaryHeader>,
}

pub fn read_packet(r: &mut impl io::Read) -> io::Result<Packet> {
    let ph = read_header(r)?;
    if ph.has_secondary_header {
        let sh = read_jpss_secondary_header(r)?;
        return Ok(Packet{
            header: ph,
            secondary_header: Some(sh),
        });
    }

    Ok(Packet{
        header: ph,
        secondary_header: None,
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
    fn test_cdstimecode() {
        // cds time from SNPP CrIS packet converted to u64
        let x: u64 = 5962765906649481435;
        let cds = decode_cds(x);

        assert_eq!(cds.days, 21184);
        assert_eq!(cds.millis, 167);
        assert_eq!(cds.micros, 219);

        let ts = cds.timestamp();
        assert_eq!(ts.to_string(), "2016-01-01 00:00:00.167219 UTC");
    }

    #[test]
    fn test_read_packet() {
        let dat: [u8; 16] = [
            // bytes from a SNPP CrIS packet
            0xd, 0x59, 0xd2, 0xab, 0xa, 0x8f,
            0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 1, 0,
        ];
        let x = &dat[0..];
        let mut r = BufReader::new(x);
        let packet = read_packet(&mut r).unwrap();

        assert_eq!(packet.header.version.to_primitive(), 0);
        assert_eq!(packet.header.is_test, false);
        assert_eq!(packet.header.has_secondary_header, true);
        assert_eq!(packet.header.apid.to_primitive(), 1369);
        assert_eq!(packet.header.sequence_flags.to_primitive(), 3);
        assert_eq!(packet.header.sequence_id.to_primitive(), 4779);
        assert_eq!(packet.header.len_minus1, 2703);

        match packet.secondary_header {
            Some(sh) => {
                assert_eq!(sh.timecode.days, 21184);
        assert_eq!(sh.timecode.millis, 167);
        assert_eq!(sh.timecode.micros, 219);
                assert_eq!(sh.num_packets, 1);
                assert_eq!(sh._spare, 0)
            },
            None => assert!(false),
        };
    }

    #[test]
    fn test_read_packet_no_secondary_header() {
        let dat: [u8; 6] = [
            // bytes from a SNPP CrIS packet with sec hdr flag set to 0
            // no secondary header!
            0x5, 0x59, 0xd2, 0xab, 0xa, 0x8f,
        ];
        let x = &dat[0..];
        let mut r = BufReader::new(x);
        let packet = read_packet(&mut r).unwrap();

        assert_eq!(packet.header.version.to_primitive(), 0);
        assert_eq!(packet.header.is_test, false);
        assert_eq!(packet.header.has_secondary_header, false);
        assert_eq!(packet.header.apid.to_primitive(), 1369);
        assert_eq!(packet.header.sequence_flags.to_primitive(), 3);
        assert_eq!(packet.header.sequence_id.to_primitive(), 4779);
        assert_eq!(packet.header.len_minus1, 2703);

        match packet.secondary_header {
            Some(_) => {
                assert!(false);
            },
            None => (),
        };
    }
}
