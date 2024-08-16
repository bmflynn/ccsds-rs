//! Timecode parsing for CCSDS Space Packet data.
//!
//! # References
//!
//! 1. [CCSDS Time Code Formats (301.0-B-4)](https://public.ccsds.org/Pubs/301x0b4e1.pdf)
//!    Section 3.2
//! 2. [EOS PM-1 Spacecraft to EOS Ground System ICD (GSFC
//!    422-11-19-03)](https://directreadout.sci.gsfc.nasa.gov/links/rsd_eosdb/PDF/ICD_Space_Ground_Aqua.pdf)
//!    Figure 5.5.1-1
//!
use serde::Serialize;

/// CCSDS Unsegmented Timecode format for the NASA EOS mission.
///
/// This is different than the standard CUC format in that the p-field extension
/// bits 1..8 contains the number of leap-seconds to convertp from TAI to UTC.
///
/// It also encodes the t-field fine-time LSB multiplier as 15.2 microseconds.
///
#[derive(Serialize, Debug)]
pub struct EosCuc {
    pub has_extension: bool,
    pub epoch: u8,
    pub num_coarse_time_octets_minus1: u8,
    pub num_fine_time_octets: u8,
    pub ext_has_ext: bool, // false

    pub leapsecs: u8,
    pub seconds: u32,
    pub sub_seconds: u32,
}

impl EosCuc {
    pub const SIZE: usize = 8;
    /// Difference between our epoch and UTC in microseconds
    pub const EPOCH_DELTA: u64 = 378_691_200_000_000;
    // Each bit is 15258 nanoseconds
    pub const LSB_MULT: u64 = 15258;

    /// Create from provivded bytes, returning `None` if decoding fails from the provided bytes.
    pub fn new(buf: &[u8]) -> Option<EosCuc> {
        // Validate buf len, but it's dynamic so we have to get some
        // p-field values to be sure
        let num_coarse = ((buf[0] >> 2) & 0x3) + 1;
        let num_fine = buf[0] & 0x3;
        if buf.len() < (num_fine + num_coarse + 2) as usize {
            return None;
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

        Some(EosCuc {
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

/// CCSDS unsegmneted timecode bytes used by NASA EOS Aqua & Terra to UTC microseconds
/// returning `None` if a value cannot be decoded from provided bytes.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn decode_eoscuc(buf: &[u8]) -> Option<u64> {
    if buf.len() < EosCuc::SIZE {
        return None;
    }

    // There is an extra byte of data before timecode
    let (bytes, _) = buf.split_at(EosCuc::SIZE);
    // we've already ensured we have enough bytes, so this won't panic

    let cuc = EosCuc::new(bytes)?;
    let mut usecs: u64 = (u64::from(cuc.seconds) + u64::from(cuc.leapsecs)) * 1_000_000;

    usecs += (u64::from(cuc.sub_seconds) * EosCuc::LSB_MULT) / 1_000;

    if usecs < EosCuc::EPOCH_DELTA {
        return None;
    }

    Some(usecs - EosCuc::EPOCH_DELTA)
}

#[cfg(test)]
mod eoscuc_tests {
    use super::*;

    #[test]
    fn test_eoscuc() {
        // bytes 7..15 from AIRS packet
        let dat: [u8; 8] = [0xae, 0x25, 0x74, 0xe3, 0xe5, 0xab, 0x5e, 0x2f];

        let ts = decode_eoscuc(&dat);
        // FIXME: Needs absolute validation against known science data.
        //        This value is taken from parsed values.
        assert_eq!(dbg!(ts), Some(1_582_401_360_367_885)); // 2020-02-22 19:56:00.367885 UTC
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cuc {
    seconds: u64,
    subseconds: u64,
}

/// Deocde a CCSDS Unsegmented Time Code. 
///
/// Note, a Cuc uses TAI time-scale with an epoch of Jan 1, 1958.
fn decode_cuc(start: usize, coarse: usize, fine: usize, buf: &[u8]) -> Option<Cuc> {
    if buf.len() < start + coarse + fine || coarse > 8 || fine > 8 {
        return None
    }
    let (x, rest) = buf.split_at(coarse);
    let mut sec_bytes = vec![0u8; 8 - coarse];
    sec_bytes.extend(x);
    let (x, _) = rest.split_at(fine);
    let mut micro_bytes = vec![0u8; 8 - fine];
    micro_bytes.extend(x);
    Some(Cuc{
        seconds: u64::from_be_bytes(sec_bytes.try_into().unwrap()),
        subseconds: u64::from_be_bytes(micro_bytes.try_into().unwrap()) * 1000,
    })
}

#[cfg(test)]
mod cuc_tests {
    use super::*;

    #[test]
    fn test_decode_cuc() {
        let dat = [0x5e, 0x96, 0x4, 0xf4, 0xab, 0x40, 0x2, 0x95];
        let tc = decode_cuc(0, 2, 4, &dat);

        assert_eq!(tc, Some(Cuc{seconds: 0, subseconds: 0}));
    }
}


/// CCSDS Day-Segmented Timecode.
///
/// This format assumes:
/// * Epoch of Jan 1, 1958
/// * 16-bit day segment
/// * 16-bit submillisecond resolution
///
/// The CDS P-field is not currently supported.
#[derive(Serialize, Debug, Clone)]
pub struct Cds {
    pub days: u16,
    pub millis: u32,
    pub micros: u16,
}

impl Cds {
    // Seconds between Unix epoch(1970) and CDS epoch(1958)
    pub const EPOCH_DELTA: u64 = 378_691_200_000_000;
    pub const SIZE: usize = 8;

    pub fn new(buf: &[u8]) -> Option<Cds> {
        if buf.len() < Self::SIZE {
            return None;
        }

        Some(Cds {
            days: u16::from_be_bytes([buf[0], buf[1]]),
            millis: u32::from_be_bytes([buf[2], buf[3], buf[4], buf[5]]),
            micros: u16::from_be_bytes([buf[6], buf[7]]),
        })
    }
}

/// CCSDS day segmented timecode bytes to UTC microseconds returning `None` if a value
/// cannot be decoded from the provided bytes.
#[must_use]
pub fn decode_cds(buf: &[u8]) -> Option<u64> {
    let cds = Cds::new(buf)?;
    let us: u64 =
        u64::from(cds.days) * 86_400_000_000 + u64::from(cds.millis) * 1000 + u64::from(cds.micros);

    if us < Cds::EPOCH_DELTA {
        None
    } else {
        Some(us - Cds::EPOCH_DELTA)
    }
}

#[cfg(test)]
mod cds_tests {
    use super::*;

    #[test]
    fn test_cds() {
        // let dat = [0x4e, 0x20, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff];
        let dat = [0x5e, 0x96, 0x4, 0xf4, 0xab, 0x40, 0x2, 0x95];

        let usecs = decode_cds(&dat).unwrap();
        assert_eq!(usecs, 1_713_481_543_488_661); // 2024-04-18 23:05:43.488661
    }
}
