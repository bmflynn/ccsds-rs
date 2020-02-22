use chrono::{DateTime, Duration, TimeZone, Utc};

use crate::error::DecodeError;

pub trait Timecode {
    fn timestamp(&self) -> DateTime<Utc>;
}

pub trait HasTimecode<T> {
    fn timecode(&self) -> Result<T, DecodeError>;
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
    pub const EPOCH_DELTA: i64 = 378_691_200;
    pub const SIZE: usize = 8;

    pub fn new(x: u64) -> CDSTimecode {
        CDSTimecode {
            days: (x >> 48 & 0xffff) as u16,
            millis: (x >> 16 & 0xffff_ffff) as u32,
            micros: (x & 0xffff) as u16,
        }
    }
}

impl Timecode for CDSTimecode {
    fn timestamp(&self) -> DateTime<Utc> {
        Utc.timestamp_nanos(
            ((self.days as u64) * 86400 * (1e9 as u64)
                + (self.millis as u64) * (1e6 as u64)
                + (self.micros as u64) * (1e3 as u64)) as i64,
        ) - Duration::seconds(CDSTimecode::EPOCH_DELTA)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cds_timecode() {
        let cds = CDSTimecode {
            days: 21184,
            millis: 167,
            micros: 219,
        };
        let ts = cds.timestamp();
        assert_eq!(ts.timestamp_millis(), 1451606400167);
    }
}
