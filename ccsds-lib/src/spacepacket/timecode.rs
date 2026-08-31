use std::collections::HashMap;

use hifitime::Epoch;

use crate::{config, Error};
use crate::{spacepacket::Packet, timecode::TimecodeDecoder};
use crate::{
    spacepacket::{Apid, PrimaryHeader},
    Result,
};

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
pub struct PacketApidTimeDecoder {
    default: Option<Box<dyn PacketTimeDecoder>>,
    decoders: HashMap<Apid, Box<dyn TimecodeDecoder>>,
}

impl Default for PacketApidTimeDecoder {
    fn default() -> Self {
        Self {
            default: None,
            decoders: HashMap::default(),
        }
    }
}

impl PacketApidTimeDecoder {
    pub fn with_default<T: PacketTimeDecoder + 'static>(mut self, decoder: T) -> Self {
        self.default = Some(Box::new(decoder));
        self
    }

    pub fn with_config(mut self, cfg: config::Config) -> Result<Self> {
        for ch in cfg.apids.iter() {
            let Some(name) = ch.timecode.as_ref() else {
                continue;
            };
            let Some(cfg) = cfg.timecodes.get(name) else {
                return Err(Error::Config("not config for timecode {name}".to_string()));
            };
            let decoder = cfg.clone().decoder()?;
            self.decoders.insert(ch.apid, decoder);
        }

        Ok(self)
    }

    pub fn add(mut self, apid: Apid, decoder: Box<dyn TimecodeDecoder>) -> Self {
        self.decoders.insert(apid, decoder);
        return self;
    }
}

impl PacketTimeDecoder for PacketApidTimeDecoder {
    fn decode(&self, packet: &Packet) -> Result<Option<Epoch>> {
        let Some(dec) = self.decoders.get(&packet.header.apid) else {
            return Ok(None);
        };
        let millis = dec.decode_unix_millis(&packet.data[PrimaryHeader::LEN..])?;
        Ok(Some(Epoch::from_unix_milliseconds(millis)))
    }
}
