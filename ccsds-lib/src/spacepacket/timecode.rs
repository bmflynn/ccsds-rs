use hifitime::Epoch;

use super::{Apid, Packet, PrimaryHeader};
use crate::prelude::*;
use std::collections::HashMap;

use crate::timecode::{decode as decode_timecode, Format};

/// Helper class to decode [hifitime::Epoch]s from [Packet]s.
///
/// It manages the match up of packet APIDs to a timecode [Format](Format), supporting a
/// default format for the case where a specific format for an APID is not found.
///
/// For sequences of packets containing only a single format only the default will be necessary.
pub struct TimecodeDecoder {
    formats: HashMap<Apid, Format>,
    default: Format,
}

impl TimecodeDecoder {
    pub fn new(default: Format) -> Self {
        Self {
            formats: HashMap::default(),
            default,
        }
    }

    /// Register `format` as a specific format to use for each of `apids`.
    pub fn register(&mut self, format: Format, apids: &[Apid]) {
        apids.iter().for_each(|a| {
            self.formats.insert(*a, format.clone());
        });
    }

    /// Decode a timecode from `packet`.
    ///
    /// # Errors
    /// If a timecode cannot be decoded for `packet` or if there is not specific format for the
    /// packet's APID and their is no default to fall back to.
    pub fn decode(&self, packet: &Packet) -> Result<Epoch> {
        let fmt = self
            .formats
            .get(&packet.header.apid)
            .unwrap_or(&self.default);
        decode_timecode(fmt, &packet.data[PrimaryHeader::LEN..])
    }
}

#[cfg(test)]
mod tests {
    use crate::spacepacket::PrimaryHeader;

    use super::*;

    #[test]
    fn test_cds() {
        let dat: Vec<u8> = vec![
            0x0b, 0x20, 0x52, 0xc4, 0x00, 0xad, 0x5c, 0xbd, 0x03, 0xc4, 0x1a, 0x6e, 0x03, 0xc9,
        ];

        let packet = Packet {
            header: PrimaryHeader::decode(&dat).unwrap(),
            data: dat,
            offset: 0,
        };
        let decoder = TimecodeDecoder::new(Format::Cds {
            num_day: 2,
            num_submillis: 2,
        });

        let timecode = decoder.decode(&packet).unwrap();

        assert_eq!(timecode.to_string(), "2023-01-01T17:33:03.470969000 UTC");
    }
}
