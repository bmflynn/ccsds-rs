//! CCSDS Frame Decoding.
//!
//! # Example
//! ```no_run
//! use std::fs::File;
//! use std::io::BufReader;
//! use ccsds::framing::*;
//!
//! const block_len: usize = 1020; // CADU length - ASM length
//! const interleave: u8 = 4;
//! const virtual_fill: usize = 0;
//! const izone_len: u32 = 0;
//! const trailer_len: u32 = 0;
//!
//! fn example() {
//!     let file = BufReader::new(File::open("snpp.dat").unwrap());
//!     let cadus = synchronize(file, SyncOpts::new(block_len)).map_while(Result::ok).map(|b| b.data);
//!     let cadus = derandomize(cadus);
//!     let rs_opts = RsOpts::new(interleave)
//!         .with_virtual_fill(virtual_fill)
//!         .with_correction(true)
//!         .with_detection(true);
//!     let cadus = reed_solomon(cadus, rs_opts)
//!         .filter(|r| r.integrity.ok());
//!     let frames = frame_decoder(cadus.map(|r| r.data));
//! }
//! ```

mod bytes;
mod packets;
mod pipeline;
mod pn;
mod reed_solomon;
mod synchronizer;

use serde::{Deserialize, Serialize};

pub use packets::PacketDemux;
pub use pipeline::*;
pub use pn::{DefaultDerandomizer, Derandomizer};
pub use reed_solomon::{DefaultReedSolomon, Integrity, ReedSolomon, RsData, RsOpts};
pub use synchronizer::{Block, ASM};

pub type Scid = u16;
pub type Vcid = u16;

/// Length of the operational control field
pub const OCF_LEN: usize = 4;
// Length of the frame error control field
pub const FEC_LEN: usize = 2;
/// Length of the frame header error control field
pub const FHEC_LEN: usize = 2;

/// Counter keeps track of the frame counter state
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Counter {
    /// Count of missing frame counts between this frame and the last received for this VCID.
    pub missing: u32,
    /// `true` if this frame has the same frame counter as the last frame received for this VCID.
    ///
    /// Such frames are not counted in [Frame::missing]. Fill frames are never marked duplicate.
    #[serde(default)]
    pub duplicate: bool,
}

/// Loose representation of a single CCSDS frame of data extracted from a Cadu.
///
/// This can generally be though of as containing frame data from a version 1 or version
/// 2 CCSDS Transfer Frame, see [VCDUHeader::decode] for details on version support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub header: VCDUHeader,
    /// Tracks the counter state when handled as part of a stream
    pub state: Option<Counter>,
    /// Frame bytes. If RS was performed upstream this will not include any parity bytes.
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

impl<S> From<S> for Frame
where
    S: AsRef<[u8]>,
{
    fn from(dat: S) -> Self {
        Frame {
            header: VCDUHeader::decode(dat.as_ref()).unwrap_or_default(),
            state: Some(Counter::default()),
            data: dat.as_ref().to_vec(),
        }
    }
}

impl Frame {
    /// Decode `dat` representing an AOS Transfer Frame into a `Frame`, or `None` if
    /// not enough bytes.
    #[must_use]
    pub fn decode(dat: &[u8]) -> Self {
        dat.into()
    }

    #[must_use]
    pub fn is_fill(&self) -> bool {
        self.header.vcid == VCDUHeader::FILL
    }

    /// Extract the MPDU bytes from this frame, or `None` if not enough bytes.
    #[must_use]
    pub fn mpdu(&self, izone_length: usize, ocf: bool, fec: bool, fhec: bool) -> Option<MPDU> {
        let start: usize = if fhec {
            VCDUHeader::LEN + izone_length + FHEC_LEN
        } else {
            VCDUHeader::LEN + izone_length
        };
        let end: usize =
            self.data.len() - if ocf { OCF_LEN } else { 0 } - if fec { FEC_LEN } else { 0 };
        let data = self.data[start..end].to_vec();

        MPDU::decode(&data)
    }
}

/// Contents of a valid VCDU header
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VCDUHeader {
    pub version: u8,
    pub scid: Scid,
    pub vcid: Vcid,
    pub counter: u32,
}

impl VCDUHeader {
    /// Construct from the provided bytes, or `None` if there are not enough bytes to decode
    /// the CCSDS header.
    ///
    /// This supports header version number 0, TM (CCSDS 132.0-B-3), and version number 1, AOS
    /// (CCSDS 732.0-B-4). The [VCDUHeader::counter] is the virtual channel frame count, which is 8
    /// bits for TM and 24 bits for AOS. The TM master channel frame count is not kept. See
    /// [VCDUHeader::counter_max].
    ///
    /// If the version is unknown or unsupported attempt to decode as v2 as it's much more common.
    #[must_use]
    pub fn decode(dat: &[u8]) -> Option<Self> {
        if dat.len() < Self::LEN {
            return None;
        }
        let ver = (dat[0] >> 6) & 0b11;
        if ver == 0 {
            return Self::decode_v1(dat);
        }
        return Self::decode_v2(dat);
    }

    /// Return true if the frame is an unknown format.
    pub fn is_opaque(&self) -> bool {
        !self.is_aos() && !self.is_tm()
    }

    /// Return true if frame is an AOS transfer frame
    pub fn is_aos(&self) -> bool {
        self.version == 1
    }

    /// Return true if frame is a TM frame
    pub fn is_tm(&self) -> bool {
        self.version == 0
    }

    /// TM Transfer Frame header CCSDS 132.0, header version number 0.
    ///
    /// Byte 2 is the master channel frame count (shared by all VCIDs) and byte 3 is the virtual
    /// channel frame count. Only the virtual channel count is used, because it is the one that
    /// increments by 1 for each frame of a single VCID.
    pub fn decode_v1(dat: &[u8]) -> Option<Self> {
        let x = u16::from_be_bytes([dat[0], dat[1]]);
        Some(VCDUHeader {
            version: 0,
            scid: ((x >> 4) & 0x3ff),
            vcid: ((x >> 1) & 0x7),
            counter: u32::from(dat[3]),
        })
    }

    /// AOS Transfer Frame header CCSDS 732.0, header version number 1.
    pub fn decode_v2(dat: &[u8]) -> Option<Self> {
        let x = u16::from_be_bytes([dat[0], dat[1]]);
        Some(VCDUHeader {
            version: dat[0] >> 6 & 0b11,
            scid: ((x >> 6) & 0xff),
            vcid: (x & 0x3f),
            counter: u32::from_be_bytes([0, dat[2], dat[3], dat[4]]),
        })
    }
}

impl VCDUHeader {
    /// VCDU header length in bytes
    pub const LEN: usize = 6;
    /// VCID indicating a fill frame
    pub const FILL: Vcid = 63;
    /// Maximum value of the 24-bit AOS VCDU counter. The next count after this value is 0.
    pub const COUNTER_MAX: u32 = 0xff_ffff;
    /// Maximum value of the 8-bit TM virtual channel frame count. The next count after this value
    /// is 0.
    pub const TM_COUNTER_MAX: u32 = 0xff;

    /// Maximum value of [VCDUHeader::counter] for this header version, after which it wraps to 0.
    #[must_use]
    pub fn counter_max(&self) -> u32 {
        if self.version == 0 {
            Self::TM_COUNTER_MAX
        } else {
            Self::COUNTER_MAX
        }
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
}

impl MPDU {
    /// MPDU first-header pointer value indicating fill data
    pub const FILL: u16 = 0x7fe;
    /// MPDU first-header pointer value indicating this MPDU does not contain a packet
    /// primary header.
    pub const NO_HEADER: u16 = 0x7ff;

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
/// `counter_max` is the largest counter value, see [VCDUHeader::counter_max]. The counter wraps
/// from `counter_max` to 0, so the result is `(cur - last - 1)` modulo `counter_max + 1`.
///
/// If `cur == last` the result is 0. A repeated counter is almost always a duplicate frame, not a
/// loss of exactly `counter_max` frames, so it is not reported as missing. This means a loss of
/// exactly that many frames cannot be detected.
///
/// Inputs are reduced modulo `counter_max + 1`, so values above `counter_max` do not overflow.
///
/// # Panics
/// If `counter_max` is `u32::MAX`.
#[must_use]
pub fn missing_frames(cur: u32, last: u32, counter_max: u32) -> u32 {
    let modulus = counter_max + 1;
    let cur = cur % modulus;
    let last = last % modulus;
    if cur == last {
        return 0;
    }
    (cur + modulus - last - 1) % modulus
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
    fn decode_vcduheader_unknown_version_decodes_as_v2() {
        let dat: Vec<u8> = vec![0b11000000, 0, 0, 0, 0, 0];

        let hdr = VCDUHeader::decode(&dat);
        assert!(hdr.is_some(), "expected some, got {:?}", hdr);
        let hdr = hdr.unwrap();
        assert_eq!(hdr.version, 3);
    }

    #[test]
    fn decode_vcduheader_is_none_when_data_too_short() {
        let zult = VCDUHeader::decode(&[0u8; 0]);
        assert!(zult.is_none());
    }

    #[test]
    fn test_missing_frames() {
        const MAX: u32 = VCDUHeader::COUNTER_MAX;
        assert_eq!(missing_frames(5, 4, MAX), 0);
        assert_eq!(missing_frames(5, 3, MAX), 1);
        // Wrap with no loss
        assert_eq!(missing_frames(0, MAX, MAX), 0);
        assert_eq!(missing_frames(MAX, MAX - 1, MAX), 0);
        // Wrap with loss
        assert_eq!(missing_frames(0, MAX - 1, MAX), 1);
        assert_eq!(missing_frames(3, MAX - 1, MAX), 4);
        assert_eq!(missing_frames(MAX, 0, MAX), MAX - 1);
        // A repeated counter is a duplicate, not a full wrap
        assert_eq!(missing_frames(0, 0, MAX), 0);
        assert_eq!(missing_frames(100, 100, MAX), 0);
        assert_eq!(missing_frames(MAX, MAX, MAX), 0);
        // Largest detectable loss, everything but the repeated counter
        assert_eq!(missing_frames(99, 100, MAX), MAX - 1);
        // Out of range input does not overflow
        assert_eq!(missing_frames(1, u32::MAX, MAX), 1);
    }

    #[test]
    fn test_missing_frames_tm_counter() {
        const MAX: u32 = VCDUHeader::TM_COUNTER_MAX;
        assert_eq!(missing_frames(1, 0, MAX), 0);
        assert_eq!(missing_frames(0, 255, MAX), 0);
        assert_eq!(missing_frames(2, 255, MAX), 2);
        assert_eq!(missing_frames(5, 3, MAX), 1);
        assert_eq!(missing_frames(7, 7, MAX), 0);
        assert_eq!(missing_frames(6, 7, MAX), 254);
    }

    #[test]
    fn decode_tm_vcduheader_uses_virtual_channel_count() {
        let dat: Vec<u8> = vec![
            // version 0, scid 0x155 (341), vcid 5, no ocf
            0x15, 0x5a, //
            0xab, // master channel count
            0x07, // virtual channel count
            0x00, 0x00,
        ];
        let header = VCDUHeader::decode(&dat).unwrap();

        assert_eq!(header.version, 0);
        assert_eq!(header.scid, 341);
        assert_eq!(header.vcid, 5);
        assert_eq!(header.counter, 7);
        assert_eq!(header.counter_max(), VCDUHeader::TM_COUNTER_MAX);
    }

    #[test]
    fn aos_counter_max() {
        let dat: Vec<u8> = vec![0x55, 0x61, 0x01, 0xe2, 0x40, 0x05];
        let header = VCDUHeader::decode(&dat).unwrap();
        assert_eq!(header.counter_max(), VCDUHeader::COUNTER_MAX);
    }
}
