//! Timecode parsing for CCSDS Space Packet data.
//!
//! ## CCSDS Day Segmented Timecode (CDS) formats
//!
//! The following data types use the CDS format.
//!
//! Satellite/Sensors using this format
//!
//! | Mission   | Satellite | Types                   |
//! |-----------|-----------|-------------------------|
//! | JPSS      | SNPP      | All sensors & S/C       |
//! | JPSS      | NOAA20    | All sensors & S/C       |
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
use crate::error::DecodeError;
use chrono::{DateTime, Duration, TimeZone, Utc};
use std::convert::TryInto;

pub trait Timecode {
    fn timecode(buf: &[u8]) -> Result<DateTime<Utc>, DecodeError>;
}

pub type TimecodeParser = dyn Fn(&[u8]) -> Result<DateTime<Utc>, DecodeError>;

/// CCSDS Unsegmented Timecode format for the NASA EOS mission.
///
/// This is different than the standard CUC format in that the p-field extension
/// bits 1..8 contains the number of leap-seconds to convertp from TAI to UTC.
///
/// It also encodes the t-field fine-time LSB multiplier as 15.2 microseconds.
///
#[derive(Debug)]
pub struct EOSCUCTimecode {
    pub has_extension: bool,
    pub epoch: u8,
    pub num_coarse_time_octets_minus1: u8,
    pub num_fine_time_octets: u8,
    pub ext_has_ext: bool, // false

    pub leapsecs: u8,
    pub seconds: u64,
    pub sub_seconds: u64,
}

impl EOSCUCTimecode {
    pub const SIZE: usize = 8;
    pub const EPOCH_DELTA: i64 = 378_691_200;

    // Each bit is 15.2 microseconds, converted here to seconds
    pub const LSB_MULT: u64 = (15.2 * 1e6) as u64;

    pub fn new(buf: &[u8]) -> Result<EOSCUCTimecode, DecodeError> {
        // Validate buf len, but it's dynamic so we have to get some
        // p-field values to be sure
        if buf.len() == 0 {
            return Err(DecodeError::TooFewBytes);
        }
        let num_coarse = ((buf[0] >> 2) & 0x3) + 1;
        let num_fine = (buf[0] & 0x3) as u8;
        if buf.len() < (num_fine + num_coarse + 2) as usize {
            return Err(DecodeError::TooFewBytes);
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

        Ok(EOSCUCTimecode {
            has_extension: (buf[0] >> 7 & 0x1) == 1,
            epoch: (buf[0] >> 4 & 0x7) as u8,
            num_coarse_time_octets_minus1: num_coarse as u8 - 1,
            num_fine_time_octets: num_fine,
            ext_has_ext: (buf[1] >> 7 & 0x1) == 1,
            leapsecs: (buf[1] & 0x7f) as u8,
            seconds: coarse_val as u64,
            sub_seconds: fine_val as u64,
        })
    }
}

pub fn parse_eoscuc_timecode(buf: &[u8]) -> Result<DateTime<Utc>, DecodeError> {
    if buf.len() < EOSCUCTimecode::SIZE {
        return Err(DecodeError::Other(format!(
            "expected {} bytes for EOSCUCTimecode, got {}",
            EOSCUCTimecode::SIZE,
            buf.len()
        )));
    }

    // There is an extra byte of data before timecode
    let (bytes, _) = buf.split_at(EOSCUCTimecode::SIZE);
    // we've already ensured we have enough bytes, so this won't panic

    let cuc = EOSCUCTimecode::new(bytes.try_into().unwrap())?;
    let secs: i64 = cuc.seconds as i64 + cuc.leapsecs as i64;
    let nanos: u32 = ((cuc.sub_seconds * EOSCUCTimecode::LSB_MULT) / 1e3 as u64) as u32;
    if secs as i64 + (nanos as i64 / 1e9 as i64) < EOSCUCTimecode::EPOCH_DELTA {
        return Err(DecodeError::Other(String::from(
            "could not decode timestamp",
        )));
    }
    let dt = Utc.timestamp_opt(secs, nanos).unwrap();
    Ok(dt - Duration::seconds(EOSCUCTimecode::EPOCH_DELTA))
}

#[cfg(test)]
mod eoscuc_tests {
    use super::*;

    #[test]
    fn test_eoscuc_timecode() {
        // bytes 7..15 from AIRS packet
        let dat: [u8; 8] = [0xae, 0x25, 0x74, 0xe3, 0xe5, 0xab, 0x5e, 0x2f];

        let ts = parse_eoscuc_timecode(&dat).unwrap();
        // FIXME: Needs absolute validation against known science data.
        //        This value is taken from parsed values.
        assert_eq!(ts.to_string(), "2020-02-22 19:56:00.366487200 UTC");
    }
}

/// CCSDS Day-Segmented Timecode
pub struct CDSTimecode {
    pub days: u16,
    pub millis: u32,
    pub micros: u16,
}

impl CDSTimecode {
    // Seconds between Unix epoch(1970) and CDS epoch(1958)
    pub const EPOCH_DELTA: i64 = 378_691_200;
    pub const SIZE: usize = 8;

    pub fn new(buf: &[u8]) -> Result<CDSTimecode, DecodeError> {
        if buf.len() < Self::SIZE {
            return Err(DecodeError::TooFewBytes);
        }

        Ok(CDSTimecode {
            days: u16::from_be_bytes([buf[0], buf[1]]),
            millis: u32::from_be_bytes(buf[2..6].try_into().unwrap()),
            micros: u16::from_be_bytes([buf[6], buf[7]]),
        })
    }
}

pub fn parse_cds_timecode(buf: &[u8]) -> Result<DateTime<Utc>, DecodeError> {
    if buf.len() < CDSTimecode::SIZE {
        return Err(DecodeError::Other(format!(
            "expected {} bytes for CDSTimecode, got {}",
            CDSTimecode::SIZE,
            buf.len()
        )));
    }
    // convert 8 bytes of time data into u64
    let (bytes, _) = buf.split_at(CDSTimecode::SIZE);

    let cds = CDSTimecode::new(bytes)?;
    let mut secs: i64 = cds.days as i64 * 86400;
    secs += cds.millis as i64 / 1e3 as i64;
    let nanos: u64 =
             // convert millis remainder to nanos
             (cds.millis as u64 * 1e6 as u64 % 1e9 as u64)
             // convert micros to nanos
              + (cds.micros as u64 * 1e3 as u64);

    Ok(Utc.timestamp_opt(secs, nanos as u32).unwrap() - Duration::seconds(CDSTimecode::EPOCH_DELTA))
}

#[cfg(test)]
mod cds_tests {
    use super::*;

    #[test]
    fn test_cds_timecode() {
        let dat = [0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff];

        let ts = parse_cds_timecode(&dat).unwrap();
        assert_eq!(ts.timestamp_millis(), 1451606400167);
    }
}
