use super::error::Error;
use super::{Apid, Packet};
use std::collections::HashMap;

use crate::timecode::{decode as decode_timecode, Error as TimecodeError, Format, Timecode};

/// Helper class to decode [Timecode](timecode::Timecode)s from [Packet]s.
///
/// It manages the match up of packet APIDs to a timecode [Format](Format), supporting a
/// default format for the case where a specific format for an APID is not found.
///
/// For sequences of packets containing only a single format only the default will be necessary.
pub struct TimecodeDecoder {
    formats: HashMap<Apid, Format>,
    default: Option<Format>,
}

impl TimecodeDecoder {
    pub fn new(default: Option<Format>) -> Self {
        Self {
            formats: HashMap::default(),
            default,
        }
    }

    fn format_for(&self, packet: &Packet) -> Option<Format> {
        match self.formats.get(&packet.header.apid) {
            Some(fmt) => Some(fmt.clone()),
            None => self.default.clone(),
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
    pub fn decode(&self, packet: &Packet) -> Result<Timecode, Error> {
        match self.format_for(packet) {
            Some(fmt) => Ok(decode_timecode(&fmt, &packet.data)?),
            None => Err(Error::Timecode(TimecodeError::Unsupported(
                "No timecode format",
            ))),
        }
    }
}
