use std::ops::Add;

use hifitime::Duration;
use hifitime::Epoch;

use super::Error;
use super::Result;
use super::TimecodeDecoder;

/// PField decodes timecodes where the PField is implied, or not present in the timecode
/// bytes.
#[derive(Debug)]
struct PField {
    /// Number of bytes of basic time
    num_basic: usize,
    /// Number of bytes of fine time
    num_fine: usize,
    /// Total length of the pfield
    len: usize,
}

impl PField {
    pub fn decode(buf: &[u8]) -> Result<PField> {
        if buf.is_empty() {
            return Err(Error::NotEnoughData);
        }
        let mut pf = PField {
            num_basic: usize::from((buf[0] >> 2) & 0b11) + 1,
            num_fine: usize::from(buf[0] & 0b11),
            len: 1,
        };
        let mut i = 0;
        while (buf[i] >> 7) & 1 == 1 {
            pf.len += 1;
            pf.num_basic += usize::from(buf[i] >> 4 & 0b11);
            pf.num_fine += usize::from(buf[i] >> 6 & 0b11);
            if buf[i] & 1 == 0 {
                break;
            }
            i += 1;
            if i >= buf.len() {
                return Err(Error::InvalidPField(
                    "pfield missing required extention bytes".to_string(),
                ));
            }
        }

        Ok(pf)
    }

    fn len(&self) -> usize {
        self.len
    }

    fn time_len(&self) -> usize {
        self.num_basic + self.num_fine
    }
}

/// CCSDS CUC [crate::TimecodeDecoder] where p-field is always present in the data
/// stream (i.e., self-identifying). The only basic time unit currently supported is
/// second.
///
/// P-field extension bytes are supported.
///
/// This decoder will fail if the p-field is **NOT** present.
pub struct PFieldDecoder {
    epoch: Epoch,
    fine_nanos: u32,
}

impl PFieldDecoder {
    /// Use the provided epoch (TAI).
    pub fn new(
        self,
        year: i32,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        nanos: u32,
    ) -> Self {
        PFieldDecoder {
            epoch: hifitime::Epoch::from_gregorian_tai(
                year, month, day, hour, minute, second, nanos,
            ),
            fine_nanos: self.fine_nanos,
        }
    }

    /// Use the provided epoch (TAI).
    pub fn with_epoch(self, secs: f64) -> Self {
        PFieldDecoder {
            epoch: hifitime::Epoch::from_unix_seconds(secs),
            fine_nanos: self.fine_nanos,
        }
    }

    pub fn with_fine_nanos(mut self, nanos: u32) -> Self {
        self.fine_nanos = nanos;
        self
    }
}

impl Default for PFieldDecoder {
    fn default() -> Self {
        Self {
            epoch: Epoch::from_gregorian_tai(1958, 1, 1, 0, 0, 0, 0),
            fine_nanos: 1,
        }
    }
}

impl TimecodeDecoder for PFieldDecoder {
    fn decode_unix_millis(&self, buf: &[u8]) -> Result<f64> {
        let pfield = PField::decode(buf)?;
        if buf.len() < pfield.len() + pfield.time_len() {
            return Err(Error::NotEnoughData);
        }
        decode_cuc(
            &buf[pfield.len()..],
            &self.epoch,
            pfield.num_basic,
            pfield.num_fine,
            self.fine_nanos,
        )
    }
}

/// CCSDS CUC [crate::TimecodeDecoder] where p-field is not present in the data
/// stream (i.e., p-field configuration is static). The only basic time unit
/// currently supported is second.
///
/// The p-field must not be present in the data stream.
pub struct Decoder {
    epoch: hifitime::Epoch,
    num_basic: usize,
    num_fine: usize,
    fine_nanos: u32,
    len: usize,
}

impl Decoder {
    /// Creates a new `Decoder` using the an epoch of Jan 1, 1958 (TAI) and the given number of basic
    /// time bytes.
    pub fn new(num_basic: usize) -> Self {
        Decoder {
            epoch: hifitime::Epoch::from_gregorian_tai(1958, 1, 1, 0, 0, 0, 0),
            num_basic,
            num_fine: 0,
            fine_nanos: 0,
            len: num_basic,
        }
    }

    /// Use the provided epoch (TAI).
    pub fn with_epoch(mut self, secs: f64) -> Self {
        self.epoch = hifitime::Epoch::from_tai_seconds(secs);
        self
    }

    /// Use the provided number of fine time config.
    ///
    /// Arguments:
    /// * `num_fine` - Number of bytes of fine time
    /// * `fine_micros` The number of microseconds for each fine time count.
    pub fn with_fine_time(mut self, num_fine: usize, fine_micros: u32) -> Self {
        self.num_fine = num_fine;
        self.fine_nanos = fine_micros;
        self.len = self.num_basic + self.num_fine;
        self
    }
}

impl TimecodeDecoder for Decoder {
    fn decode_unix_millis(&self, buf: &[u8]) -> Result<f64> {
        if buf.len() < self.len {
            return Err(Error::NotEnoughData);
        }
        decode_cuc(
            buf,
            &self.epoch,
            self.num_basic,
            self.num_fine,
            self.fine_nanos,
        )
    }
}

fn decode_cuc(
    buf: &[u8],
    epoch: &Epoch,
    num_basic: usize,
    num_fine: usize,
    fine_nanos: u32,
) -> Result<f64> {
    let (x, rest) = buf.split_at(num_basic);
    let mut day_bytes = vec![0u8; 8 - num_basic];
    day_bytes.extend(x);
    let coarse = u64::from_be_bytes(
        day_bytes
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

    let fine = fine as f64;
    let mult = if fine_nanos == 0 {
        1.0
    } else {
        f64::from(fine_nanos)
    };
    let fine_nanos = (fine * mult).trunc();

    // TODO: Handle precision loss
    let dur = Duration::compose(0, 0, 0, 0, coarse, 0, 0, fine_nanos as u64);
    Ok(epoch.clone().add(dur).to_unix_milliseconds())
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn eos_cuc() {
        // NASA EOS Spacecraft (GBAD) data
        let buf = vec![0x7d, 0xb5, 0xbf, 0x2f, 0x80, 0x1f];

        let decoder = Decoder::new(4).with_fine_time(2, 15200);
        let got = decoder.decode_unix_millis(&buf).unwrap();

        let expected = Epoch::from_str("2024-10-31T10:49:19.498544800 TAI")
            .unwrap()
            .to_unix_milliseconds();

        assert_eq!(got, expected);
    }
}
