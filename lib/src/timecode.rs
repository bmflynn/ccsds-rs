//! Time code parsing.
//!
//! Reference: [CCSDS Time Code Formats](https://public.ccsds.org/Pubs/301x0b4e1.pdf)

use crate::error::{Error, TimecodeError};

use serde::Serialize;

/// A Time Code represented as a number of nanoseconds from the CCSDS epoch of Jan 1, 1958.
///
/// For CDS format, this timecode will be correct for leap-seconds, For CUC format this timecode
/// will not be corrected for leap-seconds.
pub type Timecode = u64;

/// UTC minus CCSDS epoch in seconds
const CCSDS_UTC_DELTA: u64 = 378_691_200;

/// CCSDS timecode format configuration.
#[derive(Clone, Debug, Serialize)]
pub enum Format {
    Cds {
        offset: usize,
        daylen: usize,
        reslen: CdsRes,
    },
    Cuc {
        offset: usize,
        /// Number of bytes of coarse(seconds) data. This will generally be 4 when using the recommended
        /// epoch of Jan 1, 1958 which provides enough seconds to 2094.
        coarselen: usize,
        /// Number of bytes of fine data
        finelen: usize,
        /// A multiplier to convert `finelen` to nanosecods.
        finemult: f32,
    },
}

/// Decodes [Timecode]s from bytes.
pub trait Decoder {
    /// Decode ``buf`` into a [Timecode] according to ``format``.
    fn decode(&self, format: Format, buf: &[u8]) -> Result<Timecode, TimecodeError>;
}

/// The resolution of the Cds The enum value is the number of bytes required of submillisecond data.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum CdsRes {
    Milli = 0,
    Micro = 2,
    //Pico = 4,
}

#[derive(Clone, Debug, PartialEq, PartialOrd, Serialize)]
pub struct Cds {
    days: u32,
    millis: u64,
    nanos: u64,
}

impl Cds {
    /// CCSDS day segmented timecode bytes to UTC microseconds returning `None` if a value
    /// cannot be decoded from the provided bytes.
    ///
    /// `daylen` is the number of bytes to use for the day length component, which can be 2, or 3.
    /// `res` represents the resloution and implies the number of bytes required for sub-milliseconds.
    ///
    /// Reference: [CCSDS Time Code Formats 301.0-B-4](https://public.ccsds.org/Pubs/301x0b4e1.pdf)
    /// Section 3.3.
    ///
    /// # Errors
    /// [Error::Unsupported] if daylen or reslen are unsupported values.
    /// [Error::Other] if the `buf` does not contain enough bytes to decode timecode.
    pub fn decode(daynum: usize, res: CdsRes, buf: &[u8]) -> Result<Cds, Error> {
        if daynum != 2 && daynum != 3 {
            return Err(super::Error::Timecode(TimecodeError::Invalid(
                "daynum must be 2 or 3".to_string(),
            )));
        }
        let want = daynum + (res.clone() as usize) + 4;
        if buf.len() < want {
            return Err(super::Error::NotEnoughData(want, buf.len()));
        }

        let (x, rest) = buf.split_at(daynum);
        let mut day_bytes = vec![0u8; 4 - daynum];
        day_bytes.extend(x);
        let days = u32::from_be_bytes(day_bytes.try_into().unwrap());

        Ok(Cds {
            days,
            millis: u64::from_be_bytes([0, 0, 0, 0, rest[0], rest[1], rest[2], rest[3]]),
            nanos: match res {
                CdsRes::Milli => 0,
                CdsRes::Micro => u64::from_be_bytes([0, 0, 0, 0, 0, 0, rest[4], rest[5]]),
            },
        })
    }
}

impl TryFrom<Cds> for Timecode {
    type Error = TimecodeError;

    fn try_from(cds: Cds) -> std::result::Result<Timecode, Self::Error> {
        Ok(cds.days as u64 * 86_400_000_000_000 + cds.millis * 1_000_000 + cds.nanos)
    }
}

/// CCSDS Level-1 Unsegmented Timecode
#[derive(Clone, Debug, PartialEq, PartialOrd, Serialize)]
pub struct Cuc {
    seconds: u64,
    fine: u64,
    fine_mult: f32,
}

impl Cuc {
    /// Deocde a CCSDS Unsegmented Time Code.
    ///
    /// # Example
    /// ```
    /// // NASA EOS Spacecraft (BGAD) data
    /// let buf = vec![0xc3, 0xaa, 0x00, 0x77, 0xae, 0x25];
    /// let cuc = Cuc::decode(4, 2, 15.2, &buf);
    /// ```
    ///
    /// # Errors
    /// [TimecodeError::Invalid] if `coarse` or `fine` are >= 8.
    /// [Error::NotEnoughData](super) if `buf` is too short.
    pub fn decode(
        coarsenum: usize,
        finenum: usize,
        fine_mult: Option<f32>,
        buf: &[u8],
    ) -> Result<Cuc, Error> {
        if coarsenum > 8 {
            return Err(Error::Timecode(TimecodeError::Invalid(
                "CUC coarse must be < 8".to_string(),
            )));
        }
        if finenum > 8 {
            return Err(Error::Timecode(TimecodeError::Invalid(
                "CUC fine must be < 8".to_string(),
            )));
        }
        if buf.len() < coarsenum + finenum {
            return Err(super::Error::NotEnoughData(coarsenum + finenum, buf.len()));
        }
        let (x, rest) = buf.split_at(coarsenum);
        let mut coarse_bytes = vec![0u8; 8 - coarsenum];
        coarse_bytes.extend(x);
        let seconds = u64::from_be_bytes(coarse_bytes.try_into().unwrap());

        let (x, _) = rest.split_at(finenum);
        let mut fine_bytes = vec![0u8; 8 - finenum];
        fine_bytes.extend(x);
        let fine = u64::from_be_bytes(fine_bytes.try_into().unwrap());

        Ok(Cuc {
            seconds,
            fine,
            fine_mult: fine_mult.unwrap_or(1.0),
        })
    }
}

impl TryFrom<Cuc> for Timecode {
    type Error = TimecodeError;

    /// Return nanoseconds since CCSDS epoch of Jan 1, 1958. [Cuc] timecodes use the TAI timescale with an
    /// epoch of Jan 1, 1958 and are not corrected for leap-seconds.
    fn try_from(cuc: Cuc) -> Result<Timecode, Self::Error> {
        let Some(nanos) = (cuc.seconds - CCSDS_UTC_DELTA).checked_mul(10u64.pow(9)) else {
            return Err(TimecodeError::Unrepresentable);
        };

        let fine = cuc.fine as f64;
        if fine.is_infinite() {
            return Err(TimecodeError::Unrepresentable);
        }
        let fine_nanos = (fine * cuc.fine_mult as f64).trunc();
        if fine_nanos > u64::MAX as f64 {
            return Err(TimecodeError::Unrepresentable);
        }
        Ok(nanos + fine_nanos as u64)
    }
}

#[cfg(test)]
mod test {
    use chrono::{DateTime, Utc};

    use super::*;

    #[test]
    fn test_eos_cuc() {
        // NASA EOS Spacecraft (BGAD) data
        // <Packet apid=957 seqid=938 stamp=2024-10-31 10:48:42.497544 size=126 offset=0>
        // let buf = vec![0xc3, 0xaa, 0x00, 0x77, 0xae, 0x25];
        let buf = vec![0x7d, 0xb5, 0xbf, 0x2f, 0x80, 0x1f];
        let cuc = Cuc::decode(4, 2, Some(15200.0), &buf).unwrap();
        let nanos = Timecode::try_from(cuc).unwrap();

        let have: DateTime<Utc> = DateTime::from_timestamp_nanos(nanos as i64);
        println!("have={have:?}");
        let expected = DateTime::parse_from_rfc3339("2024-10-31T10:49:19.498544800Z").unwrap();

        assert_eq!(have, expected);
    }
}
