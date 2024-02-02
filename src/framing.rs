use std::borrow::Borrow;
use std::io::Read;
use std::sync::mpsc::{channel, sync_channel, Receiver};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::pn::{decode as pn_decode, PNDecoder};
use crate::rs::{DefaultReedSolomon, RSState, ReedSolomon};
use crate::synchronizer::{Synchronizer, ASM};
use serde::{Deserialize, Serialize};

pub type SCID = u16;
pub type VCID = u16;

/// Spacecraft Reed-solomon configuration
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RSConfig {
    pub interleave: usize,
    pub correctable: usize,
    pub vfill_length: usize,
}

/// Spacecraft framing configuration.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Framing {
    pub asm: Vec<u8>,
    /// Length of the frame contained within a CADU, not including the ASM or
    /// any Reed-Solomon parity bytes.
    ///
    /// This length, along with the [Self::izone_length] and [Self::trailer_length] will effectively
    /// define the length of an MPDU.
    pub frame_length: usize,
    pub pseudo_randomized: bool,
    pub izone_length: usize,
    pub trailer_length: usize,
    pub rs: Option<RSConfig>,
}

impl Framing {
    /// Returns the expected length of CADU which will include the ASM and the
    /// length of the Reed-Solomon code block.
    ///
    /// So, for example, with standard RS(223/255) with an interleave of 4 this
    /// will return 1024, which is 4 bytes for the ASM, 128 bits for the Reed-Solomon
    /// code block and the frame bytes.
    #[must_use]
    pub fn cadu_len(&self) -> usize {
        let rslen = match self.rs {
            Some(ref rs) => (rs.correctable * 2) * rs.interleave,
            None => 0,
        };
        self.asm.len() + self.frame_length + rslen
    }

    /// Return the length of a MPDU. This will be the ``Self::frame_length`` minus any bytes
    /// for the insert zone or trailer.
    #[must_use]
    pub fn mpdu_len(&self) -> usize {
        self.frame_length - self.izone_length - self.trailer_length
    }
}

pub const VCID_FILL: VCID = 63;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct VCDUHeader {
    pub version: u8,
    pub scid: SCID,
    pub vcid: VCID,
    pub counter: u32,
    pub replay: bool,
    pub cycle: bool,
    pub counter_cycle: u8,
}

impl VCDUHeader {
    /// VCDU header length in bytes
    pub const LEN: usize = 6;

    /// Maximum value for the zero-based VCDU counter before rollover;
    pub const COUNTER_MAX: u32 = 0xff_ffff;

    /// Construct from the provided bytes.
    ///
    /// # Panics
    /// - If `dat` does not contain enough bytes to create the header
    #[must_use]
    pub fn decode(dat: &Vec<u8>) -> Self {
        assert!(
            dat.len() >= Self::LEN,
            "vcdu header requires {} bytes, got {}",
            Self::LEN,
            dat.len()
        );

        let x = u16::from_be_bytes([dat[0], dat[1]]);
        VCDUHeader {
            version: (dat[0] >> 6) & 0x3,
            scid: ((x >> 6) & 0xff),
            vcid: (x & 0x3f),
            counter: u32::from_be_bytes([0, dat[2], dat[3], dat[4]]),
            replay: (dat[5] >> 7) & 0x1 == 1,
            cycle: (dat[5] >> 6) & 0x1 == 1,
            counter_cycle: dat[5] & 0xf,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn decode_vcduheader() {
        let dat: Vec<u8> = vec![
            0x55, 0x61, // version 1, scid 85, vcid 33
            0x01, 0xe2, 0x40, // counter 123456
            0x05, // replay:false, frame count usage:false, frame-count-cycle:5
            0x01, 0x02, 0x03, // insert zone
            0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0xaa, // first-header-pointer 682
        ];

        let header = VCDUHeader::decode(&dat);

        assert_eq!(header.version, 1);
        assert_eq!(header.scid, 85);
        assert_eq!(header.vcid, 33);
        assert_eq!(header.counter, 123_456);
        assert!(!header.replay);
        assert!(!header.cycle);
        assert_eq!(header.counter_cycle, 5);
    }

    #[test]
    fn decode_vcduheader_panics_when_data_too_short() {
        let zult = std::panic::catch_unwind(|| VCDUHeader::decode(&vec![0u8; 0]));
        assert!(zult.is_err(), "decode should panic with too little data");
    }

    #[test]
    fn decode_frame() {
        let dat: Vec<u8> = vec![
            0x55, 0x61, // version 1, scid 85, vcid 33
            0x01, 0xe2, 0x40, // counter 123456
            0x05, // replay:false, frame count usage:false, frame-count-cycle:5
            0x00, 0x00, 0x00,
        ];
        let expected_len = dat.len();
        let frame = Frame::decode(dat);

        assert_eq!(frame.data.len(), expected_len);
    }
}

pub struct MPDU {
    // the offset of the header minus 1
    first_header: u16,
    data: Vec<u8>,
}

impl MPDU {
    /// MPDU first-header pointer value indicating fill data
    pub const FILL: u16 = 0x7fe;
    /// MPDU first-header pointer value indicating this MPDU does not contain a packet
    /// primary header.
    pub const NO_HEADER: u16 = 0x7ff;

    #[must_use]
    pub fn decode(data: &[u8]) -> Self {
        let x = u16::from_be_bytes([data[0], data[1]]);
        MPDU {
            first_header: x & 0x7ff,
            data: data.to_vec(),
        }
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

#[derive(Debug)]
pub struct Frame {
    pub header: VCDUHeader,
    /// All frame data bytes, including header
    pub data: Vec<u8>,
}

impl Frame {
    #[must_use]
    pub fn decode(dat: Vec<u8>) -> Self {
        let header = VCDUHeader::decode(&dat);
        Frame { header, data: dat }
    }

    #[must_use]
    pub fn is_fill(&self) -> bool {
        self.header.vcid == VCID_FILL
    }

    #[must_use]
    pub fn mpdu(&self, izone_length: usize, trailer_length: usize) -> MPDU {
        let start: usize = VCDUHeader::LEN + izone_length;
        let end: usize = self.data.len() - trailer_length;
        let data = self.data[start..end].to_vec();

        MPDU::decode(&data)
    }
}

pub struct DecodedFrame {
    pub frame: Frame,
    pub missing: u32,
    pub rsstate: RSState,
}

/// Provides [Frame]s based on configuration provided by the parent ``FrameDecoderBuilder``.
pub struct DecodedFrameIter {
    done: bool,
    jobs: Receiver<Receiver<(Frame, RSState)>>,
    errch: Receiver<std::io::Error>,
    handle: Option<JoinHandle<()>>,
    last: Option<u32>,
}

impl DecodedFrameIter {
    // Return the error that caused decoding to fail early, or None. This will always
    // return [None] if the iterator is not finished.
    #[must_use]
    pub fn err(&self) -> Option<std::io::Error> {
        if !self.done {
            return None;
        }
        match self.errch.recv() {
            Ok(err) => Some(err),
            _ => None,
        }
    }
}

impl Iterator for DecodedFrameIter {
    type Item = DecodedFrame;

    fn next(&mut self) -> Option<Self::Item> {
        // recv block current thread until data is available.
        match self.jobs.recv() {
            Err(_) => {
                self.done = true;
                self.handle
                    .take()
                    .expect("bad state, handle should not be None")
                    .join()
                    .expect("reedsolomon thread paniced");
                None
            }
            Ok(rx) => {
                let (frame, rsstate) = rx.recv().expect("failed to receive future");
                let missing = match self.last {
                    Some(last) => missing_frames(frame.header.counter, last),
                    None => 0,
                };
                self.last = Some(frame.header.counter);

                Some(DecodedFrame {
                    frame,
                    missing,
                    rsstate,
                })
            }
        }
    }
}

/// Builds a ``DecodedFrameIter`` that will return all frames decoded from the stream read
/// from reader.
///
/// Reads are only performed when a [Frame] is requested from the returned iterator, i.e.,
/// when ``Iterator::next`` is called. More bytes than the size of the frame may be read if the
/// underlying stream is not synchronized.
///
/// Frames will generated in the order in which they occur in the original byte stream.
///
/// IO is performed concurrently so the iterator can be returned immediately. All PN
/// and RS decoding is likewise performed concurrently.
pub struct FrameDecoderBuilder {
    asm: Vec<u8>,
    interleave: u8,
    cadu_length: usize,
    buffer_size: usize,

    pn_decoder: Option<PNDecoder>,
    reed_solomon: Option<Box<dyn ReedSolomon + Sync + 'static>>,
    reed_solomon_threads: usize,
}

impl FrameDecoderBuilder {
    /// Default number of frames to buffer in memory while waiting for RS.
    pub const DEFAULT_BUFFER_SIZE: usize = 1024;

    /// Create a new ``DecodedFrameIter`` with default values suitable for decoding most (all?)
    /// CCSDS compatible frame streams.
    ///
    /// `cadu_length` should be the length of the attached sync marker and the Reed-Solomon
    /// code block, if the stream uses Reed-Solomon, or the length of the transfer frame.
    /// You _must_ include the length of the RS codeblock if the stream uses RS, even if you
    /// have disabled RS FEC.
    ///
    /// Given the `interleave` for a spacecraft, for most cases all that should be necessary
    /// is the following:
    /// ```
    /// use ccsds::FrameDecoderBuilder;
    /// let r = &[0u8; 1][..]; // implements Read
    /// let decoded_frames = FrameDecoderBuilder::new(1024)
    ///     .reed_solomon_interleave(4)
    ///     .build(r);
    /// ```
    ///
    /// It is possible, however, to twidle with default implementations using the provided
    /// builder functions.
    pub fn new(cadu_length: usize) -> Self {
        FrameDecoderBuilder {
            cadu_length,
            interleave: 0,
            asm: ASM.to_vec(),
            pn_decoder: Some(pn_decode),
            reed_solomon: None,
            reed_solomon_threads: 0, // Let rayon decide
            buffer_size: Self::DEFAULT_BUFFER_SIZE,
        }
    }

    /// Limits the number of block waiting in memory for RS.
    /// See ``FrameDecoderBuilder::DEFAULT_BUFFER_SIZE``.
    #[must_use]
    pub fn buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Set the CADU Attached Sync Marker used to synchronize the incoming stream.
    /// Defaults to [ASM];
    #[must_use]
    pub fn attached_sync_marker(mut self, asm: &[u8]) -> Self {
        self.asm = asm.to_vec();
        self
    }

    /// Use the default Reed-Solomon with the specified interleave value.
    ///
    /// For more control over Reed-Solomon, see ``reed_solomon``.
    ///
    /// # Panics
    /// If `interleave` is 0.
    ///
    /// [reed_solomon]: Self::reed_solomon
    #[must_use]
    pub fn reed_solomon_interleave(self, interleave: u8) -> Self {
        self.reed_solomon(Some(Box::new(DefaultReedSolomon {})), interleave)
    }

    /// Set the Reed-Solomon per-CADU implementation to use. Defaults to ``DefaultReedSolomon``.
    ///
    /// # Panics
    /// If `interleave` is 0.
    #[must_use]
    pub fn reed_solomon(mut self, rs: Option<Box<dyn ReedSolomon + Sync>>, interleave: u8) -> Self {
        assert!(interleave > 0, "invalid rs interleave; must be > 0");
        self.reed_solomon = rs;
        self.interleave = interleave;
        self
    }

    /// Set the number of threads to use for Reed-Solomon. If not explicitly set, the
    /// number of threads is chosen automatically.
    #[must_use]
    pub fn reed_solomon_threads(mut self, num: usize) -> Self {
        self.reed_solomon_threads = num;
        self
    }

    /// Set PN implementation.
    #[must_use]
    pub fn pn_decode(mut self, pn: Option<PNDecoder>) -> Self {
        self.pn_decoder = pn;
        self
    }

    /// Returns a ``DecodedFrameIter`` configured according to the provided options.
    ///
    /// # Panics
    /// If the processing thread cannot be spawned.
    #[must_use]
    pub fn build(self, reader: impl Read + Send + 'static) -> DecodedFrameIter {
        // A "job" in this context is the processing of 1 block. Receivers on which the
        // RS results are delivered as sent on this channel, one for each block.
        let (jobs_tx, jobs_rx) = sync_channel(self.buffer_size);
        // Channel for delivering any non EOF error that may terminate processing early.
        let (err_tx, err_rx) = channel();

        let interleave = self.interleave;
        let pn_decoder = self.pn_decoder;

        // Do IO (Read/synchronize) in the background where each synchronized block or
        // CADU will be submitted to a thread pool such that the PN and RS can run in the
        // background.
        let handle = thread::Builder::new()
            .name("reedsolomon".into())
            .spawn(move || {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(self.reed_solomon_threads)
                    .build()
                    .expect("failed to construct RS threadpool with requested number of threads");

                let jobs_tx = jobs_tx.clone();
                let synchronizer =
                    Synchronizer::new(reader, &self.asm, self.cadu_length - self.asm.len());
                let reed_solomon = Arc::new(self.reed_solomon);

                for block in synchronizer {
                    if let Err(err) = block {
                        err_tx.send(err).expect("failed to send error");
                        break; // exit thread
                    }
                    let mut block = block.unwrap();

                    let reed_solomon = reed_solomon.clone();
                    let (future_tx, future_rx) = channel();
                    // spawn_fifo makes sure the frame order is maintained
                    pool.spawn_fifo(move || {
                        // Only do PN if not None
                        if let Some(pn_decode) = pn_decoder {
                            block = pn_decode(&block);
                        }
                        // Only do RS if not None
                        let (dat, state) = match reed_solomon.borrow() {
                            Some(rs) => rs.correct_codeblock(&block, interleave),
                            None => (block, RSState::NotPerformed),
                        };

                        let frame = Frame::decode(dat);
                        future_tx
                            .send((frame, state))
                            .expect("failed to send frame");
                    });
                    jobs_tx
                        .send(future_rx)
                        .expect("failed to send future receiver");
                }
            })
            .unwrap();

        DecodedFrameIter {
            done: false,
            jobs: jobs_rx,
            errch: err_rx,
            handle: Some(handle),
            last: None,
        }
    }
}

/// Calculate the number of missing frame sequence counts.
///
/// `cur` is the current frame counter. `last` is the frame counter seen before `cur`.
///
/// # Panics
/// If the frame couner goes out of bounds
#[must_use]
pub fn missing_frames(cur: u32, last: u32) -> u32 {
    let expected = if last + 1 > VCDUHeader::COUNTER_MAX {
        0
    } else {
        last + 1
    };
    if cur != expected {
        if last + 1 > cur {
            return cur + VCDUHeader::COUNTER_MAX - last;
        }
        return cur - last - 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn fixture_path(name: &str) -> PathBuf {
        let mut path = PathBuf::from(file!());
        path.pop();
        path.pop();
        path.push(name);
        path
    }

    #[test]
    fn test_decode_single_frame() {
        let mut dat: Vec<u8> = vec![
            0x67, 0x50, 0x96, 0x30, 0xbc, 0x80, // VCDU Header
            0x07, 0xff, // MPDU header indicating no header
        ];
        dat.resize(892, 0xff);

        assert_eq!(dat.len(), 892);

        let frame = Frame::decode(dat);
        assert_eq!(frame.header.scid, 157);
        assert_eq!(frame.header.vcid, 16);

        let mpdu = frame.mpdu(0, 0);
        assert!(!mpdu.is_fill());
        assert!(
            mpdu.first_header == MPDU::NO_HEADER,
            "expected {} got {}",
            MPDU::NO_HEADER,
            mpdu.first_header
        );
        assert!(!mpdu.has_header());
    }

    #[test]
    fn test_decode_frames() {
        let fpath = fixture_path("tests/fixtures/snpp_7cadus_2vcids.dat");
        let reader = fs::File::open(fpath).unwrap();

        let frames: Vec<DecodedFrame> = FrameDecoderBuilder::new(1024)
            .reed_solomon_interleave(4)
            .build(reader)
            .collect();
        assert_eq!(frames.len(), 7, "expected frame count doesn't match");
        for (idx, df) in frames.iter().enumerate() {
            assert_eq!(df.frame.header.scid, 157);
            if idx < 3 {
                assert_eq!(df.frame.header.vcid, 16);
            } else {
                assert_eq!(df.frame.header.vcid, 6);
            }
        }
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
