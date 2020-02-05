extern crate packed_struct;
#[macro_use]
extern crate packed_struct_codegen;

use chrono::{DateTime, LocalResult, TimeZone, Utc};
use packed_struct::prelude::*;

use std::fs::File;
use std::io;
use std::io::{BufReader, BufRead, Read, Seek};

#[derive(PackedStruct, Debug)]
#[packed_struct(endian="msb", bit_numbering="msb0")]
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
const PRIMARY_HEADER_SIZE: usize = 6;

fn decode_header(r: &mut impl io::Read) -> Result<PrimaryHeader, io::Error> {
    let mut buf: [u8; PRIMARY_HEADER_SIZE] = [0; PRIMARY_HEADER_SIZE];
    r.read_exact(&mut buf)?;
    match PrimaryHeader::unpack(&buf) {
        Ok(h) => Ok(h),
        Err(err) => Err(io::Error::new(io::ErrorKind::Other, err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_header() {
        let dat: [u8; 6] = [
            0xd, 0x59, 0xd2, 0xab, 0xa, 0x8f
        ];
        let x = &dat[0..];
        let mut r = BufReader::new(x);
        let ph = decode_header(&mut r).unwrap();

        assert_eq!(ph.version.to_primitive(), 0);
        assert_eq!(ph.is_test, false);
        assert_eq!(ph.has_secondary_header, true);
        assert_eq!(ph.apid.to_primitive(), 1369);
        assert_eq!(ph.sequence_flags.to_primitive(), 3);
        assert_eq!(ph.sequence_id.to_primitive(), 4779);
        assert_eq!(ph.len_minus1, 2703);
    }

}

#[derive(PackedStruct)]
#[packed_struct(endian = "msb", bit_numbering = "msb0")]
pub struct CDSTimecode {
    #[packed_field(size_bits = "16")]
    days: u16,
    #[packed_field(size_bits = "32")]
    millis: u32,
    #[packed_field(size_bits = "16")]
    micros: u16,
}
const CDS_TIMECODE_SIZE: usize = 8;

fn decode_cds(dat: u64) -> DateTime<Utc> {
    let days = dat >> 48 & 0xffff;
    let millis = dat >> 16 & 0xffffffff;
    let micros = dat & 0xffff;

    Utc.timestamp_nanos(
        (days * 86400 * (1e9 as u64) + millis * (1e6 as u64) + micros * (1e3 as u64)) as i64,
    )
}


fn main() -> std::io::Result<()> {
    let fp = File::open("snpp_cris.dat")?;
    let mut reader = io::BufReader::new(fp);

    loop {
        let ph = decode_header(&mut reader)?;
        println!("{:?}", ph);
        reader.seek(io::SeekFrom::Current((ph.len_minus1+1) as i64))?;
    }
}
