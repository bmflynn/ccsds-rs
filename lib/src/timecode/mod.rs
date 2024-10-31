//! Time code parsing.
//!
//! Reference: [Time Code Formats](https://public.ccsds.org/Pubs/301x0b4e1.pdf)
mod cds;
mod cuc;
mod error;
mod leapsecs;

pub use super::error::*;
pub use cds::Cds;
use chrono::{DateTime, Utc};

/// Represents a timecode in UTC.
pub struct Timecode {
    pub days: u32,
    pub millis: u32,
    pub picos: u64,
    pub scale: Scale,
}

/// Defines the time-scale for a timecode
pub enum Scale {
    /// TAI based on provided epoch; not adjusted for leap-seconds
    Tai { epoch: DateTime<Utc> },
    /// UTC using standard 1 Jan, 1970; adjusted for leap-seconds
    Utc,
}

/// CCSDS timecode format configuration
pub enum Format {
    Cds {
        daylen: usize,
        sublen: usize,
    },
    Cuc {
        seconds_len: usize,
        fine_len: usize,
        /// Multiplier to convert from fine time to picoseconds of the day
        fine_mult: u64,
    },
}

/// Decodes [[Timecode]]s from bytes.
pub trait Decoder {
    /// Decode ``buf`` into a [[Timecode]] according to ``format``.
    fn decode(&self, format: Format, buf: &[u8]) -> Result<Timecode>;
}
