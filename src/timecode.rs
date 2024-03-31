//! Timecode parsing for CCSDS Space Packet data.
//!
//! ## CCSDS Day Segmented Timecode (CDS) formats
//!
//! Satellite/Sensors using this format (not a complete list!).
//!
//! | Mission   | Satellite | Types                   |
//! |-----------|-----------|-------------------------|
//! | JPSS      | SNPP      | All sensors & S/C       |
//! | JPSS      | NOAA20    | All sensors & S/C       |
//! | JPSS      | NOAA21    | All sensors & S/C       |
//! | EOS       | Aqua      | *MODIS (Sci & Engr)     |
//! | EOS       | Aqua      | *CERES                  |
//!
//! * EOS GIIS packet format as documented in reference 1
//!
//! ## CCSDS Unsegmented Timecode (CUC) Formats
//!
//! The following instruments use some form of CUC format, with different number
//! of field bits and starting at different offsets into the user data zone.
//!
//! | Mission   | Satellite | Types           | P-field len | T-Field len | Start byte |
//! |-----------|-----------|-----------------|-------------|-------------|------------|
//! | EOS       | Aqua      | All sensor HK   | 2           | 6           | 1          |
//! | EOS       | Aqua      | Most S/C        | 2           | 6           | 0          |
//! | EOS       | Aqua      | AMSU-E          | 1           | 5           | 1          |
//!
//! * CUC formats are used for the EOS GIRD and S/C packet formats as
//!   documented in reference 1
//!
//!
//! # References
//!
//! 1. [CCSDS Timecode Formats (301.0-B-4)](https://public.ccsds.org/Pubs/301x0b4e1.pdf)
//!    Section 3.2
//! 2. [EOS PM-1 Spacecraft to EOS Ground System ICD (GSFC
//!    422-11-19-03)](https://directreadout.sci.gsfc.nasa.gov/links/rsd_eosdb/PDF/ICD_Space_Ground_Aqua.pdf)
//!    Figure 5.5.1-1
//!
use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::Serialize;
use std::convert::TryInto;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed to create timecode from provided bytes")]
    Parse(String),
    #[error("buffer too short to create timecode")]
    BufferTooShort,
}

pub trait Timecode {
    /// Convert bytes to ``DateTime<Utc>``.
    ///
    /// # Errors
    /// If the bytes cannot be to a timecode
    fn timecode(buf: &[u8]) -> Result<DateTime<Utc>, Error>;
}

pub type Parser = dyn Fn(&[u8]) -> Result<DateTime<Utc>, Error>;

/// CCSDS Unsegmented Timecode format for the NASA EOS mission.
///
/// This is different than the standard CUC format in that the p-field extension
/// bits 1..8 contains the number of leap-seconds to convertp from TAI to UTC.
///
/// It also encodes the t-field fine-time LSB multiplier as 15.2 microseconds.
///
#[derive(Serialize, Debug)]
pub struct EOSCUC {
    pub has_extension: bool,
    pub epoch: u8,
    pub num_coarse_time_octets_minus1: u8,
    pub num_fine_time_octets: u8,
    pub ext_has_ext: bool, // false

    pub leapsecs: u8,
    pub seconds: u32,
    pub sub_seconds: u32,
}

impl EOSCUC {
    pub const SIZE: usize = 8;
    pub const EPOCH_DELTA: i64 = 378_691_200;

    // Each bit is 15.2 microseconds
    pub const LSB_MULT: u32 = 1520_0000;

    /// Create from provivded bytes.
    ///
    /// # Errors
    /// If the dynamic number of bytes are not available.
    ///
    /// # Panics
    /// On overflow converting decoded numeric types, or if there are not the correct number
    /// of bytes.
    pub fn new(buf: &[u8]) -> Result<EOSCUC, Error> {
        // Validate buf len, but it's dynamic so we have to get some
        // p-field values to be sure
        let num_coarse = ((buf[0] >> 2) & 0x3) + 1;
        let num_fine = buf[0] & 0x3;
        if buf.len() < (num_fine + num_coarse + 2) as usize {
            return Err(Error::BufferTooShort);
        }

        // figure out mask for coarse time
        let mut coarse_tmp: [u8; 8] = [0u8; 8];
        for (i, j) in ((8 - num_coarse)..8).zip(0..num_coarse) {
            coarse_tmp[i as usize] = buf[(2 + j) as usize];
        }
        let coarse_val = u64::from_be_bytes(coarse_tmp);

        // figure out mask for fine time
        let mut fine_tmp: [u8; 8] = [0u8; 8];
        // some iter magic to make the indexing easier
        for (i, j) in ((8 - num_fine)..8).zip(0..num_fine) {
            fine_tmp[i as usize] = buf[(2 + num_coarse + j) as usize];
        }
        let fine_val = u64::from_be_bytes(fine_tmp);

        Ok(EOSCUC {
            has_extension: (buf[0] >> 7 & 0x1) == 1,
            epoch: (buf[0] >> 4 & 0x7),
            num_coarse_time_octets_minus1: num_coarse - 1,
            num_fine_time_octets: num_fine,
            ext_has_ext: (buf[1] >> 7 & 0x1) == 1,
            leapsecs: (buf[1] & 0x7f),
            seconds: u32::try_from(coarse_val).expect("failed to convert coarse seconds to u32"),
            sub_seconds: u32::try_from(fine_val).expect("failed to convert fine time to u32"),
        })
    }
}

/// CCSDS unsegmneted timecode bytes used by NASA EOS Aqua & Terra to ``DateTime``.
///
/// # Errors
/// ``Error::BufferTooShort`` if there are not enough bytes.
///
/// # Panics
/// On overflow converting decoded numeric types
pub fn decode_eoscuc(buf: &[u8]) -> Result<DateTime<Utc>, Error> {
    if buf.len() < EOSCUC::SIZE {
        return Err(Error::BufferTooShort);
    }

    // There is an extra byte of data before timecode
    let (bytes, _) = buf.split_at(EOSCUC::SIZE);
    // we've already ensured we have enough bytes, so this won't panic

    let cuc = EOSCUC::new(bytes)?;
    let secs: u32 = cuc.seconds + u32::from(cuc.leapsecs);
    let nanos: u32 =
        u32::try_from((u64::from(cuc.sub_seconds) * u64::from(EOSCUC::LSB_MULT)) / 1000u64)
            .unwrap();
    if (i64::from(secs) + i64::from(nanos / 1_000_000_000u32)) < EOSCUC::EPOCH_DELTA {
        return Err(Error::Parse("could not decode timestamp".to_owned()));
    }
    let dt = Utc.timestamp_opt(i64::from(secs), nanos).unwrap();
    Ok(dt - Duration::seconds(EOSCUC::EPOCH_DELTA))
}

#[cfg(test)]
mod eoscuc_tests {
    use super::*;

    #[test]
    fn test_eoscuc() {
        // bytes 7..15 from AIRS packet
        let dat: [u8; 8] = [0xae, 0x25, 0x74, 0xe3, 0xe5, 0xab, 0x5e, 0x2f];

        let ts = decode_eoscuc(&dat).unwrap();
        // FIXME: Needs absolute validation against known science data.
        //        This value is taken from parsed values.
        assert_eq!(ts.to_string(), "2020-02-22 19:56:00.366487200 UTC");
    }
}

/// CCSDS Day-Segmented Timecode
#[derive(Serialize, Debug, Clone)]
pub struct CDS {
    pub days: u16,
    pub millis: u32,
    pub micros: u16,
}

impl CDS {
    // Seconds between Unix epoch(1970) and CDS epoch(1958)
    pub const EPOCH_DELTA: i64 = 378_691_200;
    pub const SIZE: usize = 8;

    pub fn new(buf: &[u8]) -> Result<CDS, Error> {
        if buf.len() < Self::SIZE {
            return Err(Error::BufferTooShort);
        }

        Ok(CDS {
            days: u16::from_be_bytes([buf[0], buf[1]]),
            millis: u32::from_be_bytes(buf[2..6].try_into().unwrap()),
            micros: u16::from_be_bytes([buf[6], buf[7]]),
        })
    }
}

/// CCSDS day segmented timecode bytes to ``DateTime``.
///
/// Returns ``Error::BufferTooShort`` if there are not enough bytes.
pub fn decode_cds(buf: &[u8]) -> Result<DateTime<Utc>, Error> {
    if buf.len() < CDS::SIZE {
        return Err(Error::BufferTooShort);
    }
    // convert 8 bytes of time data into u64
    let (bytes, _) = buf.split_at(CDS::SIZE);

    let cds = CDS::new(bytes)?;
    let mut secs: i64 = i64::from(cds.days) * 86400;
    secs += i64::from(cds.millis) / 1000i64;
    let nanos: u64 =
             // convert millis remainder to nanos
             (u64::from(cds.millis) * 1_000_000 % 1_000_000_000)
             // convert micros to nanos
              + (u64::from(cds.micros) * 1000);

    Ok(Utc.timestamp_opt(secs, nanos.try_into().unwrap()).unwrap()
        - Duration::seconds(CDS::EPOCH_DELTA))
}

#[cfg(test)]
mod cds_tests {
    use super::*;

    #[test]
    fn test_cds() {
        let dat = [0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff];

        let ts = decode_cds(&dat).unwrap();
        assert_eq!(ts.timestamp_millis(), 1_451_606_400_167);
    }

    #[test]
    fn test_cds_overflow() {
        let dat = [0, 1, 2, 3, 4, 5, 6, 7];

        let ts = decode_cds(&dat).unwrap();
        assert_eq!(ts.timestamp_millis(), -378_571_047_930, "{ts:?}");
    }
}
