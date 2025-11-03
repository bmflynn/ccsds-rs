//! CCSDS Frame Decoding.
//!
//! # Example
//! ```no_run
//! use std::fs::File;
//! use std::io::BufReader;
//! use ccsds::framing::*;
//!
//! let block_len = 1020; // CADU length - ASM length
//! let interleave = 4;
//! let virtual_fill = 0;
//! let izone_len = 0;
//! let trailer_len = 0;
//!
//! let file = BufReader::new(File::open("snpp.dat").unwrap());
//! let cadus = synchronize(file, SyncOpts::new(block_len));
//! let cadus = derandomize(cadus);
//! let frames = frame_decoder(cadus);
//! let rs_opts = RsOpts::new(interleave)
//!     .with_virtual_fill(virtual_fill)
//!     .with_correction(true)
//!     .with_detection(true)
//!     .with_num_threads(0); // use all CPUs
//! let frames = reed_solomon(frames, rs_opts)
//!     .filter(|frame| match frame.integrity {
//!         Some(ref val) => val.ok(),
//!         None => false,
//!     });
//! ```

mod bytes;
mod ocf;
mod packets;
mod pipeline;
mod pn;
mod reed_solomon;
mod synchronizer;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

pub use pipeline::*;
pub use pn::{DefaultDerandomizer, Derandomizer};
pub use reed_solomon::{DefaultReedSolomon, Integrity, ReedSolomon};
pub use synchronizer::{Block, ASM};

pub type Scid = u16;
pub type Vcid = u16;
pub type Cadu = Block;

/// Loose representation of a single frame of data extracted from a Cadu.
///
/// This can generally be though of as containing from data from a version 1 or version
/// 2 CCSDS Transfer Frame, see [VCDUHeader::decode] for details on version support.
///
/// TM Transfer Frames (CCSDS 132.0-B-3) are not supported.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Frame {
    /// This frames header data
    pub header: VCDUHeader,
    /// Count of missing frame counts between this frame and the last received for this VCID.
    pub missing: u32,
    /// Integrity checking disposition, if peformed, [Option::None] otherwise.
    pub integrity: Option<Integrity>,
    /// Frame bytes. If integrity checking was performed and failed, e.g., not [Integrity::Ok] or
    /// [Integrity::Corrected], this will also include any check symbols and therefore potentially
    /// be longer than the expected frame length.
    #[cfg_attr(feature = "serde", serde(with = "serde_bytes"))]
    pub data: Vec<u8>,
}

impl Frame {
    /// Decode `dat` representing an AOS Transfer Frame into a `Frame`, or `None` if
    /// not enough bytes.
    #[must_use]
    pub fn decode(dat: Vec<u8>) -> Option<Self> {
        let header = VCDUHeader::decode(&dat)?;
        Some(Frame {
            header,
            missing: 0,
            integrity: None,
            data: dat,
        })
    }

    #[must_use]
    pub fn is_fill(&self) -> bool {
        self.header.vcid == VCDUHeader::FILL
    }

    /// Extract the MPDU bytes from this frame, or `None` if not enough bytes.
    #[must_use]
    pub fn mpdu(&self, izone_length: usize, trailer_length: usize) -> Option<MPDU> {
        let start: usize = VCDUHeader::LEN + izone_length;
        let end: usize = self.data.len() - trailer_length;
        let data = self.data[start..end].to_vec();

        MPDU::decode(&data)
    }
}

/// Contents of a valid VCDU header
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct VCDUHeader {
    pub version: u8,
    pub scid: Scid,
    pub vcid: Vcid,
    pub counter: u32,
}

impl VCDUHeader {
    /// VCDU header length in bytes
    pub const LEN: usize = 6;
    /// VCID indicating a fill frame
    pub const FILL: Vcid = 63;
    /// Maximum value for the zero-based VCDU counter before rollover;
    pub const COUNTER_MAX: u32 = 0xff_ffff - 1;

    /// Construct from the provided bytes, or `None` if there are not enough bytes
    /// or the version is not supported.
    ///
    /// This supports version 1 (CCSDS 732.0-B-1) and version 2 (CCSDS 132.0-B-3). For
    /// version 2 the master channel frame count and virtual channel frame count are
    /// combined into a single 32b counter.
    ///
    /// If the version is unknown or unsupported, `None` is returned.
    #[must_use]
    pub fn decode(dat: &[u8]) -> Option<Self> {
        if dat.len() < Self::LEN {
            return None;
        }

        let ver = (dat[0] >> 6) & 0x3;
        match ver {
            0 => Self::decode_v1(dat),
            1 => Self::decode_v2(dat),
            // Unknown or unsupported version
            _ => None,
        }
    }

    /// TM Transfer Frame header CCSDS 132.0
    fn decode_v1(dat: &[u8]) -> Option<Self> {
        let x = u16::from_be_bytes([dat[0], dat[1]]);
        Some(VCDUHeader {
            version: 0,
            scid: ((x >> 4) & 0x3ff),
            vcid: ((x >> 1) & 0x7),
            counter: u32::from_be_bytes([0, 0, dat[2], dat[3]]),
        })
    }

    /// AOS Transfer Frame header CCSDS 732.0
    fn decode_v2(dat: &[u8]) -> Option<Self> {
        let x = u16::from_be_bytes([dat[0], dat[1]]);
        Some(VCDUHeader {
            version: 1,
            scid: ((x >> 6) & 0xff),
            vcid: (x & 0x3f),
            counter: u32::from_be_bytes([0, dat[2], dat[3], dat[4]]),
        })
    }
}

/// MPDU contained within a [Frame].
#[derive(Clone)]
pub struct MPDU {
    // the offset of the header minus 1
    first_header: u16,
    data: Vec<u8>,
}

impl std::fmt::Debug for MPDU {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MPDU {{ fill:{} fhp:{:#x} }}",
            self.is_fill(),
            self.header_offset()
        )
    }
}

impl MPDU {
    /// MPDU first-header pointer value indicating fill data
    pub const FILL: u16 = 0x7fe;
    /// MPDU first-header pointer value indicating this MPDU does not contain a packet
    /// primary header.
    pub const NO_HEADER: u16 = 0x7ff;

    /// Decode `data` into a ``VCDUHeader``.
    #[must_use]
    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 2 {
            return None;
        }
        let x = u16::from_be_bytes([data[0], data[1]]);

        Some(MPDU {
            first_header: x & 0x7ff,
            data: data.to_vec(),
        })
    }

    #[must_use]
    pub fn is_fill(&self) -> bool {
        self.first_header == Self::FILL
    }

    #[must_use]
    pub fn has_header(&self) -> bool {
        self.first_header != Self::NO_HEADER
    }

    /// Get the payload bytes from this MPDU.
    ///
    /// # Panics
    /// If there are not enough bytes to construct the MPDU
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        assert!(self.data.len() >= 2, "mpdu data too short");
        &self.data[2..]
    }

    #[must_use]
    pub fn header_offset(&self) -> usize {
        self.first_header as usize
    }
}

/// Calculate the number of missing frame sequence counts.
///
/// `cur` is the current frame counter. `last` is the frame counter seen before `cur`.
/// `cur` will be greater than `last` except in the case of a wrap.
#[must_use]
pub fn missing_frames(cur: u32, last: u32) -> u32 {
    if cur == last {
        return VCDUHeader::COUNTER_MAX;
    }

    let expected = if last == VCDUHeader::COUNTER_MAX {
        0
    } else {
        last + 1
    };

    if cur == expected {
        0
    } else {
        if cur < last {
            return VCDUHeader::COUNTER_MAX - last + cur;
        }
        cur - last - 1
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn decode_vcduheader() {
        let dat: Vec<u8> = vec![
            0x55, 0x61, // version 2 (0x01), scid 85, vcid 33
            0x01, 0xe2, 0x40, // counter 123456
            0x05, // replay:false, frame count usage:false, frame-count-cycle:5
            0x01, 0x02, 0x03, // insert zone
            0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0xaa, // first-header-pointer 682
        ];

        let header = VCDUHeader::decode(&dat).unwrap();

        assert_eq!(header.version, 1);
        assert_eq!(header.scid, 85);
        assert_eq!(header.vcid, 33);
        assert_eq!(header.counter, 123_456);
    }

    #[test]
    fn decode_vcduheader_unsupported_version_is_none() {
        let dat: Vec<u8> = vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff];

        let hdr = VCDUHeader::decode(&dat);
        assert!(hdr.is_none(), "expected none, got {:?}", hdr);
    }

    #[test]
    fn decode_vcduheader_is_err_when_data_too_short() {
        let zult = VCDUHeader::decode(&[0u8; 0]);
        assert!(zult.is_none());
    }

    #[test]
    fn test_missing_frames() {
        assert_eq!(missing_frames(5, 4), 0);
        assert_eq!(missing_frames(5, 3), 1);
        assert_eq!(missing_frames(0, VCDUHeader::COUNTER_MAX), 0);
        assert_eq!(missing_frames(0, VCDUHeader::COUNTER_MAX - 1), 1);
        assert_eq!(missing_frames(0, 0), VCDUHeader::COUNTER_MAX);
    }
}
