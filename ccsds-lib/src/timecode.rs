//! Time code parsing.
//!
//! Reference: [CCSDS Time Code Formats](https://public.ccsds.org/Pubs/301x0b4e1.pdf)
use hifitime::{Duration, Epoch};

#[cfg(feature = "python")]
use pyo3::prelude::*;

use crate::prelude::*;
use serde::Serialize;

/// Number of seconds between the 1958 and 1900
const CCSDS_HIFIEPOCH_DELTA_SECS: u64 = 1830297600;
/// Default number of bytes for the CDS milliseconds field
const NUM_CDS_MILLIS_OF_DAY_BYTES: usize = 4;
/// Max number of u64 nanoseconds that can be cast to f64 w/o precision loss
const MAX_FINE_NANOS: f64 = 4_503_599_627_370_496.0;

/// CCSDS timecode format configuration.
#[cfg_attr(feature = "python", pyclass)]
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

/// Decode `buf` into [hifitime::Epoch].
///
/// # Errors
/// [Error::NotEnoughData] if there is not enough data for the provided format, or
/// [Error::TimecodeConfig] if a timecode cannot be constructected for the provided format. This
/// will usually be due to providing unsupported timecode values in a format field.
pub fn decode(format: &Format, buf: &[u8]) -> Result<Epoch> {
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

fn decode_cds(num_day: usize, num_submillis: usize, buf: &[u8]) -> Result<Epoch> {
    let want = num_day + num_submillis + NUM_CDS_MILLIS_OF_DAY_BYTES;
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

    let millis = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]);
    let nanos = match num_submillis {
        0 => 0,
        2 => u32::from_be_bytes([0, 0, rest[4], rest[5]]) * 1_000,
        4 => u32::from_be_bytes([rest[4], rest[5], rest[6], rest[7]]) * 1_000_000,
        _ => {
            return Err(Error::TimecodeConfig(format!(
                "Number of CDS sub-millisecond must be 0, 2, or 4; got {num_submillis}"
            )))
        }
    };

    let dur = Duration::compose(
        0,
        days as u64,
        0,
        0,
        // Add in delta to get to hifi epoch
        CCSDS_HIFIEPOCH_DELTA_SECS,
        millis as u64,
        0,
        nanos as u64,
    );
    Ok(Epoch::from_utc_duration(dur))
}

fn decode_cuc(
    num_coarse: usize,
    num_fine: usize,
    fine_mult: Option<f32>,
    buf: &[u8],
) -> Result<Epoch> {
    if !(1..=4).contains(&num_coarse) {
        return Err(Error::TimecodeConfig(
            "Number of CUC coarse bytes must be 1 to 4".to_string(),
        ));
    }
    if !(0..=3).contains(&num_fine) {
        return Err(Error::TimecodeConfig(
            "Number of CUC fine bytes must be 0 to 3".to_string(),
        ));
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

    // Convert to hifi epoch
    let coarse = coarse + CCSDS_HIFIEPOCH_DELTA_SECS;

    let fine = fine as f64;
    let fine_nanos = (fine * fine_mult.unwrap_or(1.0) as f64).trunc();
    if fine_nanos > MAX_FINE_NANOS {
        return Err(Error::Overflow);
    }
    let dur = Duration::compose(0, 0, 0, 0, coarse, 0, 0, fine_nanos as u64);
    Ok(Epoch::from_tai_duration(dur))
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

        assert_eq!(cds, expected, "timecode={:?}", cds);
    }

    #[test]
    fn eos_cuc() {
        // NASA EOS Spacecraft (BGAD) data
        let buf = vec![0x7d, 0xb5, 0xbf, 0x2f, 0x80, 0x1f];
        let cuc = decode_cuc(4, 2, Some(15200.0), &buf).unwrap();

        let expected = Epoch::from_str("2024-10-31T10:49:19.498544800 TAI").unwrap();

        assert_eq!(cuc, expected);
    }
}
