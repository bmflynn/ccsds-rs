use std::collections::HashMap;

use hifitime::Epoch;
use spacecrafts::Spacecraft;

use crate::{
    framing::Vcid,
    spacepacket::{Apid, Packet, PrimaryHeader},
    timecode::TimecodeDecoder,
    Result,
};
use crate::{timecode::decoder_with_config, Error};

pub trait PacketTimeDecoder {
    /// Decode [Epoch] from a [Packet].
    ///
    /// This makes an assumption that the pfield is implied and not included in
    /// the packet bytes, and also that the timecode format details can be determined
    /// soley by the packet metadata (APID).
    ///
    /// Arguments:
    /// * `packet` - The packet to decode the timestamp for
    fn decode(&self, packet: &Packet) -> Result<Option<Epoch>>;
}

/// A [PacketTimeDecoder] that uses a static timecode format.
pub struct StaticTimeDecoder<T: TimecodeDecoder>(T);

impl<T> StaticTimeDecoder<T>
where
    T: TimecodeDecoder,
{
    pub fn new(decoder: T) -> Self {
        StaticTimeDecoder(decoder)
    }
}

impl<T> PacketTimeDecoder for StaticTimeDecoder<T>
where
    T: TimecodeDecoder,
{
    fn decode(&self, packet: &Packet) -> Result<Option<Epoch>> {
        let millis = self
            .0
            .decode_unix_millis(&packet.data[PrimaryHeader::LEN..])?;
        Ok(Some(Epoch::from_unix_milliseconds(millis)))
    }
}

/// ApidTimecodeDecoder is a [PacketTimeDecoder] for decoding times from packet secondary headers
/// per-APID.
///
/// The same APID may be used on more than one VCID. Decoders added with
/// [Self::with_spacecraft] or [Self::add_for_vcid] are looked up by `(vcid, apid)` using
/// [Packet::vcid]. When the packet has no VCID, or there is no `(vcid, apid)` match, the
/// decoder for the APID alone is used (the first one added for that APID).
#[derive(Default)]
pub struct PacketApidTimeDecoder {
    default: Option<Box<dyn PacketTimeDecoder>>,
    by_vcid_apid: HashMap<(Vcid, Apid), Box<dyn TimecodeDecoder>>,
    by_apid: HashMap<Apid, Box<dyn TimecodeDecoder>>,
}

impl PacketApidTimeDecoder {
    pub fn with_default<T: PacketTimeDecoder + 'static>(mut self, decoder: T) -> Self {
        self.default = Some(Box::new(decoder));
        self
    }

    pub fn with_spacecraft(mut self, sc: &Spacecraft) -> Result<Self> {
        for ch in sc.apids.iter() {
            let Some(name) = ch.timecode.as_ref() else {
                continue;
            };
            let Some(cfg) = sc.timecodes.get(name) else {
                return Err(Error::Config(format!("no config for timecode {name}")));
            };
            self.by_vcid_apid
                .insert((ch.vcid, ch.apid), decoder_with_config(cfg)?);
            // First config for an APID is the fallback when the VCID is unknown
            if let std::collections::hash_map::Entry::Vacant(e) = self.by_apid.entry(ch.apid) {
                e.insert(decoder_with_config(cfg)?);
            }
        }

        Ok(self)
    }

    /// Add a decoder used for `apid` on any VCID, and for packets with no VCID.
    pub fn add(mut self, apid: Apid, decoder: Box<dyn TimecodeDecoder>) -> Self {
        self.by_apid.insert(apid, decoder);
        self
    }

    /// Add a decoder used only for `apid` on `vcid`.
    pub fn add_for_vcid(
        mut self,
        vcid: Vcid,
        apid: Apid,
        decoder: Box<dyn TimecodeDecoder>,
    ) -> Self {
        self.by_vcid_apid.insert((vcid, apid), decoder);
        self
    }
}

impl PacketTimeDecoder for PacketApidTimeDecoder {
    fn decode(&self, packet: &Packet) -> Result<Option<Epoch>> {
        let apid = packet.header.apid;
        let by_vcid = packet
            .vcid
            .and_then(|vcid| self.by_vcid_apid.get(&(vcid, apid)));
        let Some(dec) = by_vcid.or_else(|| self.by_apid.get(&apid)) else {
            return Ok(None);
        };
        let millis = dec.decode_unix_millis(&packet.data[PrimaryHeader::LEN..])?;
        Ok(Some(Epoch::from_unix_milliseconds(millis)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(f64);

    impl TimecodeDecoder for Fixed {
        fn decode_unix_millis(
            &self,
            _buf: &[u8],
        ) -> std::result::Result<f64, crate::timecode::Error> {
            Ok(self.0)
        }
    }

    fn packet(vcid: Option<Vcid>) -> Packet {
        #[rustfmt::skip]
        let dat: &[u8] = &[
            0xd, 0x59, 0xc0, 0x01, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];
        let mut p = Packet::decode(dat).unwrap();
        p.vcid = vcid;
        p
    }

    #[test]
    fn same_apid_different_vcid() {
        let dec = PacketApidTimeDecoder::default()
            .add_for_vcid(1, 1369, Box::new(Fixed(1000.0)))
            .add_for_vcid(2, 1369, Box::new(Fixed(2000.0)))
            .add(1369, Box::new(Fixed(3000.0)));

        let ms = |p: &Packet| dec.decode(p).unwrap().unwrap();
        assert_eq!(ms(&packet(Some(1))), Epoch::from_unix_milliseconds(1000.0));
        assert_eq!(ms(&packet(Some(2))), Epoch::from_unix_milliseconds(2000.0));
        // unknown vcid and no vcid fall back to the apid-only decoder
        assert_eq!(ms(&packet(Some(9))), Epoch::from_unix_milliseconds(3000.0));
        assert_eq!(ms(&packet(None)), Epoch::from_unix_milliseconds(3000.0));
    }

    #[test]
    fn no_decoder() {
        let dec = PacketApidTimeDecoder::default().add_for_vcid(1, 1369, Box::new(Fixed(1.0)));
        assert!(dec.decode(&packet(None)).unwrap().is_none());
    }
}
