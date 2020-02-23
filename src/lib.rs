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
pub mod timecode;
pub mod packet;
pub mod stream;

use packed_struct::prelude::*;

use std::io;

/// CCSDS Primary Header
///
/// The primary header format is common to all CCSDS space packets.
///
#[derive(PackedStruct, Debug)]
#[packed_struct(endian = "msb", bit_numbering = "msb0")]
pub struct PrimaryHeader {
    #[packed_field(bits = "0:2")]
    version: Integer<u8, packed_bits::Bits3>,
    #[packed_field(size_bits = "1")]
    type_flag: u8,
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
        let mut buf = [0u8; PrimaryHeader::SIZE];
        r.read_exact(&mut buf)?;
        Ok(PrimaryHeader::unpack(&buf)?)
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

        assert_eq!(ph.version.to_primitive(), 0);
        assert_eq!(ph.type_flag, 0);
        assert_eq!(ph.has_secondary_header, true);
        assert_eq!(ph.apid.to_primitive(), 1369);
        assert_eq!(ph.sequence_flags.to_primitive(), 3);
        assert_eq!(ph.sequence_id.to_primitive(), 4779);
        assert_eq!(ph.len_minus1, 2703);
    }

}
