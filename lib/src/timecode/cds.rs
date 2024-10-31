use serde::Serialize;

use super::error::{Error, Result};

/// CCSDS Day-Segmented Timecode with epoch of Jan 1, 1958.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Cds {
    /// Days since Jan 1, 1958
    pub days: u32,
    /// Picosecond of the day
    pub picos: u64,
}

impl Cds {
    // Seconds between Unix epoch(1970) and CDS epoch(1958)
    pub const EPOCH_DELTA: u64 = 378_691_200_000_000;

    /// CCSDS day segmented timecode bytes to UTC microseconds returning `None` if a value
    /// cannot be decoded from the provided bytes.
    ///
    /// `daylen` is the number of bytes to use for the day length component, which can be 2, or 3.
    /// `reslen` is the number of bytes to use for the resolution, or submillisecond, component, which
    /// can be 0, 2 or 4.
    ///
    /// Reference: [CCSDS Timecode Formats 301.0-B-4](https://public.ccsds.org/Pubs/301x0b4e1.pdf)
    /// Section 3.3.
    ///
    /// # Errors
    /// [Error::Unsupported] if daylen or reslen are unsupported values.
    /// [Error::Other] if the `buf` does not contain enough bytes to decode timecode.
    pub fn decode(daynum: usize, resnum: usize, buf: &[u8]) -> Result<Cds> {
        let (days, millis, picos) = match (daynum, resnum) {
            (2, 0) => {
                if buf.len() < 6 {
                    return Err(Error::Other(crate::Error::TooShort(6, buf.len())));
                }
                (
                    u32::from_be_bytes([0, 0, buf[0], buf[1]]),
                    u64::from_be_bytes([0, 0, 0, 0, buf[2], buf[3], buf[4], buf[5]]),
                    0,
                )
            }
            (2, 2) => {
                if buf.len() < 8 {
                    return Err(Error::Other(crate::Error::TooShort(8, buf.len())));
                }
                (
                    u32::from_be_bytes([0, 0, buf[0], buf[1]]),
                    u64::from_be_bytes([0, 0, 0, 0, buf[2], buf[3], buf[4], buf[5]]),
                    u64::from_be_bytes([0, 0, 0, 0, 0, 0, buf[6], buf[7]]) * 1000 * 1000,
                )
            }
            (2, 4) => {
                if buf.len() < 10 {
                    return Err(Error::Other(crate::Error::TooShort(10, buf.len())));
                }
                (
                    u32::from_be_bytes([0, 0, buf[2], buf[3]]),
                    u64::from_be_bytes([0, 0, 0, 0, buf[2], buf[3], buf[4], buf[5]]),
                    u64::from_be_bytes([0, 0, 0, 0, buf[6], buf[7], buf[8], buf[9]]),
                )
            }
            (3, 0) => {
                if buf.len() < 7 {
                    return Err(Error::Other(crate::Error::TooShort(7, buf.len())));
                }
                (
                    u32::from_be_bytes([0, buf[0], buf[1], buf[2]]),
                    u64::from_be_bytes([0, 0, 0, 0, buf[3], buf[4], buf[5], buf[6]]),
                    0,
                )
            }
            (3, 2) => {
                if buf.len() < 9 {
                    return Err(Error::Other(crate::Error::TooShort(9, buf.len())));
                }
                (
                    u32::from_be_bytes([0, buf[0], buf[1], buf[2]]),
                    u64::from_be_bytes([0, 0, 0, 0, buf[3], buf[4], buf[5], buf[6]]),
                    u64::from_be_bytes([0, 0, 0, 0, 0, 0, buf[7], buf[8]]) * 1000 * 1000,
                )
            }
            (3, 4) => {
                if buf.len() < 11 {
                    return Err(Error::Other(crate::Error::TooShort(11, buf.len())));
                }
                (
                    u32::from_be_bytes([0, buf[0], buf[1], buf[2]]),
                    u64::from_be_bytes([0, 0, 0, 0, buf[3], buf[4], buf[5], buf[6]]),
                    u64::from_be_bytes([0, 0, 0, 0, buf[7], buf[8], buf[9], buf[10]]),
                )
            }
            _ => return Err(Error::Invalid(format!("CDS d{daynum} r{resnum}"))),
        };

        Ok(Cds {
            days,
            picos: millis * 1000 * 1000 * 1000 + picos,
        })
    }

    /// Returns the number of microseconds since Jan 1, 1970 UTC.
    pub fn timestamp_micros(&self) -> Option<u64> {
        let Some(day_micros) = (self.days as u64).checked_mul(86_400_000_000) else {
            return None;
        };
        // micros since 1958
        let micros = day_micros + (self.picos / 1000 / 1000);

        if micros < Cds::EPOCH_DELTA {
            return None;
        }
        Some(micros - Cds::EPOCH_DELTA)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cds() {
        let dat = [0x5e, 0x96, 0x4, 0xf4, 0xab, 0x40, 0x2, 0x95];

        assert_eq!(Cds { days: 0, picos: 0 }, Cds::decode(2, 2, &dat).unwrap(),);
    }
}
