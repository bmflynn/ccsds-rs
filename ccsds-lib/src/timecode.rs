//! Time code parsing.
//!
//! Reference: [CCSDS Time Code Formats](https://public.ccsds.org/Pubs/301x0b4e1.pdf)
use hifitime::{Duration, Epoch};

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Unsupported configuration for timecode: {0}")]
    Unsupported(&'static str),
    #[error("Overflow")]
    Overflow,
    #[error("Underflow")]
    Underflow,
    #[error("Not enough bytes")]
    NotEnoughData { actual: usize, minimum: usize },
}

/// CCSDS Level-1 Timecode implementations.
///
/// Level-1 implies the timecodes use the recommended CCSDS epoch of Jan 1, 1958.
#[derive(Clone, Debug, Serialize)]
#[non_exhaustive]
pub enum Timecode {
    /// CCSDS Day Segmneted time code format.
    ///
    /// This format assumes the epoch of Jan 1, 1958.
    ///
    /// See: [CCSDS Time Code Formats 301.0-B-4](https://public.ccsds.org/Pubs/301x0b4e1.pdf), Section 3.3.
    #[non_exhaustive]
    Cds { days: u32, millis: u32, nanos: u32 },
    /// CCSDS Unsegmented time code format.
    ///
    /// This format assumes the coarse time unit is seconds and epoch is Jan 1, 1958.
    ///
    /// See: [CCSDS Time Code Formats 301.0-B-4](https://public.ccsds.org/Pubs/301x0b4e1.pdf), Section 3.2.
    #[non_exhaustive]
    Cuc {
        coarse: u64,
        fine: u64,
        /// This will be some factor used to convert `fine` to nanoseconds. This may be necessary
        /// for missions such as NASA EOS where the spacecraft telemetry CUC format only uses 2
        /// bytes for fine time and a multiplier of 15.2 microseconds for each fine time value. In
        /// this case `fine_mult` would be 15200.0.
        fine_mult: Option<f32>,
    },
}

impl Timecode {
    /// Number of seconds between the 1958 and 1900
    const CCSDS_HIFIEPOCH_DELTA_SECS: u64 = 1830297600;
    /// Default number of bytes for the CDS milliseconds field
    const NUM_CDS_MILLIS_OF_DAY_BYTES: usize = 4;

    /// Max number of u64 nanoseconds that can be cast to f64 w/o precision loss
    const MAX_FINE_NANOS: f64 = 4_503_599_627_370_496.0;

    /// Decode this timecode into a [hifitime::Epoch].
    ///
    /// # Errors
    /// [Error::Overflow] If numeric conversions would result in overflow or precision loss
    pub fn epoch(&self) -> Result<Epoch, Error> {
        match self {
            Timecode::Cds {
                days,
                millis,
                nanos,
            } => {
                let dur = Duration::compose(
                    0,
                    *days as u64,
                    0,
                    0,
                    // Add in delta to get to hifi epoch
                    Self::CCSDS_HIFIEPOCH_DELTA_SECS,
                    *millis as u64,
                    0,
                    *nanos as u64,
                );
                Ok(Epoch::from_utc_duration(dur))
            }
            Timecode::Cuc {
                coarse,
                fine,
                fine_mult,
            } => {
                // Convert to hifi epoch
                let coarse = coarse + Self::CCSDS_HIFIEPOCH_DELTA_SECS;

                let fine = *fine as f64;
                let fine_nanos = (fine * fine_mult.unwrap_or(1.0) as f64).trunc();
                if fine_nanos > Self::MAX_FINE_NANOS {
                    return Err(Error::Overflow);
                }
                let dur = Duration::compose(0, 0, 0, 0, coarse, 0, 0, fine_nanos as u64);
                Ok(Epoch::from_tai_duration(dur))
            }
        }
    }

    /// Return the number of nanoseconds since Jan 1, 1958
    ///
    /// # Errors
    /// [Error::Overflow] If numeric conversions would result in overflow or precision loss
    pub fn nanos(&self) -> Result<u64, Error> {
        match self {
            Timecode::Cds {
                days,
                millis,
                nanos,
            } => {
                let Some(day_nanos) = (*days as u64).checked_mul(86_400_000_000_000) else {
                    return Err(Error::Overflow);
                };
                let Some(milli_nanos) = (*millis as u64).checked_mul(1_000_000) else {
                    return Err(Error::Overflow);
                };
                Ok(day_nanos + milli_nanos + *nanos as u64)
            }
            Timecode::Cuc {
                coarse,
                fine,
                fine_mult,
            } => {
                // Convert to hifi epoch
                let Some(coarse_nanos) = coarse.checked_mul(1_000_000_000) else {
                    return Err(Error::Overflow);
                };

                let fine = *fine as f64;
                let fine_nanos = (fine * fine_mult.unwrap_or(1.0) as f64).trunc();
                if fine_nanos > Self::MAX_FINE_NANOS {
                    return Err(Error::Overflow);
                }
                Ok(coarse_nanos + fine_nanos as u64)
            }
        }
    }
}

/// CCSDS timecode format configuration.
#[derive(Clone, Debug, Serialize)]
#[non_exhaustive]
pub enum Format {
    /// Day segmented timecode parameters.
    ///
    /// Valid combinations are:
    /// |`num_day`|`num_submillis`| |
    /// |---|---|---|
    /// |2|0|No sub-milliseconds|
    /// |2|2|Microsecond resolution|
    /// |2|4|Picosecond resolution|
    /// |3|0|No sub-milliseconds|
    /// |3|2|Microsecond resolution|
    /// |3|4|Picosecond resolution|
    Cds {
        num_day: usize,
        num_submillis: usize,
    },
    /// Unsegmented timecode parameters.
    ///
    /// Valid `num_coarse` is between 1 and 4.
    /// Valid `num_fine` is between 0 and 3.
    Cuc {
        num_coarse: usize,
        num_fine: usize,
        /// Factor by which to multiple `num_fine` to produce nanoseconds.
        fine_mult: Option<f32>,
    },
}

/// Decode `buf` into [Timecode::Cuc].
///
/// # Errors
/// - [Error::Unsupported] If `num_coarse` and `num_fine` is not a valid combination
/// - [Error::Unsupported] if a timecode cannot be created from `buf` according to `format`
/// - [Error::Overflow] or [Error::Underflow] if the numeric conversions don't work out.
pub fn decode(format: &Format, buf: &[u8]) -> Result<Timecode, Error> {
    match format {
        Format::Cds {
            num_day,
            num_submillis,
        } => decode_cds(*num_day, *num_submillis, buf),
        Format::Cuc {
            num_coarse,
            num_fine,
            fine_mult,
        } => decode_cuc(*num_coarse, *num_fine, *fine_mult, buf),
    }
}

fn decode_cds(num_day: usize, num_submillis: usize, buf: &[u8]) -> Result<Timecode, Error> {
    let want = num_day + num_submillis + Timecode::NUM_CDS_MILLIS_OF_DAY_BYTES;
    if buf.len() < want {
        return Err(Error::NotEnoughData {
            actual: buf.len(),
            minimum: want,
        });
    }

    let (x, rest) = buf.split_at(num_day);
    let mut day_bytes = vec![0u8; 4 - num_day];
    day_bytes.extend(x);
    let days = u32::from_be_bytes([day_bytes[0], day_bytes[1], day_bytes[2], day_bytes[3]]);

    Ok(Timecode::Cds {
        days,
        millis: u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]),
        nanos: match num_submillis {
            0 => 0,
            2 => u32::from_be_bytes([0, 0, rest[4], rest[5]]) * 1_000,
            4 => u32::from_be_bytes([rest[4], rest[5], rest[6], rest[7]]) * 1_000_000,
            _ => panic!("number of sub-millisecond must be 0, 2, or 4; got {num_submillis}"),
        },
    })
}

fn decode_cuc(
    num_coarse: usize,
    num_fine: usize,
    fine_mult: Option<f32>,
    buf: &[u8],
) -> Result<Timecode, Error> {
    if !(1..=4).contains(&num_coarse) {
        return Err(Error::Unsupported("Invalid CUC coarse config"));
    }
    if !(0..=3).contains(&num_fine) {
        return Err(Error::Unsupported("Invalid CUC fine config"));
    }
    if buf.len() < num_coarse + num_fine {
        return Err(Error::NotEnoughData {
            minimum: num_coarse + num_fine,
            actual: buf.len(),
        });
    }
    let (x, rest) = buf.split_at(num_coarse);
    let mut coarse_bytes = vec![0u8; 8 - num_coarse];
    coarse_bytes.extend(x);
    let coarse = u64::from_be_bytes(
        coarse_bytes
            .try_into()
            .expect("to be able to convert vec to array"),
    );

    let (x, _) = rest.split_at(num_fine);
    let mut fine_bytes = vec![0u8; 8 - num_fine];
    fine_bytes.extend(x);
    let fine = u64::from_be_bytes(
        fine_bytes
            .try_into()
            .expect("to be able to convert vec to array"),
    );

    Ok(Timecode::Cuc {
        coarse,
        fine,
        fine_mult,
    })
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn cds() {
        let buf = vec![0x5f, 0x5b, 0x00, 0x00, 0x06, 0x94, 0x02, 0x07];
        let cds = decode_cds(2, 2, &buf).unwrap();

        let expected = Epoch::from_str("2024-11-01T00:00:01.684519Z").unwrap();

        assert_eq!(cds.epoch().unwrap(), expected, "timecode={:?}", cds);
    }

    #[test]
    fn eos_cuc() {
        // NASA EOS Spacecraft (BGAD) data
        let buf = vec![0x7d, 0xb5, 0xbf, 0x2f, 0x80, 0x1f];
        let cuc = decode_cuc(4, 2, Some(15200.0), &buf).unwrap();
        let epoch = cuc.epoch().unwrap();

        let expected = Epoch::from_str("2024-10-31T10:49:19.498544800 TAI").unwrap();

        assert_eq!(epoch, expected);
    }
}
