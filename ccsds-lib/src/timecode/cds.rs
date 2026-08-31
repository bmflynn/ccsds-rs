use std::ops::Add;

use hifitime::Duration;
use hifitime::Epoch;

use super::Error;
use super::Result;
use super::TimecodeDecoder;

pub struct PField {
    /// Number of day bytes
    num_day: usize,
    /// Number of submilli bytes
    num_submillis: usize,
}

impl PField {
    pub fn new(num_day: usize, num_submillis: usize) -> Result<Self> {
        match num_day {
            2 | 3 => (),
            x => {
                return Err(Error::InvalidPField(format!(
                    "cds num_day must be 2 or 3; got {x}"
                )))
            }
        }
        match num_submillis {
            0 | 1 | 2 => (),
            x => {
                return Err(Error::InvalidPField(format!(
                    "submillis must be 0, 1, 2; got {x}"
                )))
            }
        }
        Ok(PField {
            num_day,
            num_submillis,
        })
    }
    pub fn decode(buf: &[u8]) -> Result<PField> {
        if buf.is_empty() {
            return Err(Error::NotEnoughData);
        }
        Self::new(
            if buf[0] >> 3 & 1 == 1 { 3 } else { 2 },
            usize::from(buf[0] >> 4u8 & 0b11),
        )
    }

    fn len(&self) -> usize {
        self.num_day + self.num_submillis + 4
    }
}

/// CCSDS CDS [crate::TimecodeDecoder] where p-field is always present in the data
/// stream (i.e., self-identifying). The only basic time unit currently supported is
/// second.
///
/// P-field extension bytes are supported.
///
/// This decoder will fail if the p-field is **NOT** present.
pub struct PFieldDecoder {
    epoch: Epoch,
}

impl PFieldDecoder {
    /// Use the provided epoch (TAI).
    pub fn with_epoch(self, secs: f64) -> Self {
        PFieldDecoder {
            epoch: hifitime::Epoch::from_unix_seconds(secs),
        }
    }
}

impl Default for PFieldDecoder {
    fn default() -> Self {
        Self {
            epoch: Epoch::from_gregorian_utc(1958, 1, 1, 0, 0, 0, 0),
        }
    }
}

impl TimecodeDecoder for PFieldDecoder {
    fn decode_unix_millis(&self, buf: &[u8]) -> Result<f64> {
        let pfield = PField::decode(buf)?;
        if buf.len() < pfield.len() {
            return Err(Error::NotEnoughData);
        }
        decode_cds(buf, &self.epoch, pfield.num_day, pfield.num_submillis)
    }
}

/// CCSDS CDS [crate::TimecodeDecoder] where p-field is not present in the data
/// stream (i.e., p-field configuration is static). The only basic time unit
/// currently supported is second.
///
/// The p-field must not be present in the data stream.
pub struct Decoder {
    epoch: hifitime::Epoch,
    num_day: usize,
    num_submillis: usize,
    len: usize,
}

impl Decoder {
    /// Creates a new `Decoder` using the an epoch of Jan 1, 1958 (UTC) and the given number of day
    /// and submilli bytes.
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
    pub fn new(num_day: usize, num_submillis: usize) -> Self {
        Decoder {
            epoch: hifitime::Epoch::from_gregorian_utc(1958, 1, 1, 0, 0, 0, 0),
            num_day,
            num_submillis,
            len: num_day + num_submillis + 4,
        }
    }

    /// Use the provided epoch (UTC).
    pub fn with_epoch(mut self, secs: f64) -> Self {
        self.epoch = hifitime::Epoch::from_unix_seconds(secs);
        self
    }
}

impl TimecodeDecoder for Decoder {
    fn decode_unix_millis(&self, buf: &[u8]) -> Result<f64> {
        if buf.len() < self.len {
            return Err(Error::NotEnoughData);
        }
        decode_cds(buf, &self.epoch, self.num_day, self.num_submillis)
    }
}

fn decode_cds(buf: &[u8], epoch: &Epoch, num_day: usize, num_submillis: usize) -> Result<f64> {
    let want = num_day + num_submillis + 4;
    if buf.len() < want {
        return Err(Error::NotEnoughData);
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
        x => {
            return Err(Error::InvalidPField(format!(
                "number of CDS sub-millisecond must be 0, 2, or 4; got {x}"
            )))
        }
    };
    let dur = Duration::from_days(days as f64)
        + Duration::from_milliseconds(millis as f64)
        + Duration::from_nanoseconds(nanos as f64);
    Ok(epoch.clone().add(dur).to_unix_milliseconds())
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn cds() {
        let buf = vec![0x5f, 0x5b, 0x00, 0x00, 0x06, 0x94, 0x02, 0x07];
        let decoder = Decoder::new(2, 2);
        let cds = decoder.decode_unix_millis(&buf).unwrap();

        let expected = Epoch::from_str("2024-11-01T00:00:01.684519Z")
            .unwrap()
            .to_unix_milliseconds();

        assert_eq!(cds, expected, "timecode={:?}", cds);
    }
}
