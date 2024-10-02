use std::collections::HashMap;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::rs::{DefaultReedSolomon, IntegrityError, RSState, ReedSolomon};
use crate::{DefaultPN, PNDecoder};
use crossbeam::channel::{bounded, unbounded, Receiver};
use serde::{Deserialize, Serialize};
use tracing::{debug, span, Level};
use typed_builder::TypedBuilder;

pub type SCID = u16;
pub type VCID = u16;

pub const VCID_FILL: VCID = 63;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
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
    pub const COUNTER_MAX: u32 = 0xff_ffff - 1;

    /// Construct from the provided bytes, or `None` if there are not enough bytes.
    #[must_use]
    pub fn decode(dat: &[u8]) -> Option<Self> {
        if dat.len() < Self::LEN {
            return None;
        }

        let x = u16::from_be_bytes([dat[0], dat[1]]);
        Some(VCDUHeader {
            version: (dat[0] >> 6) & 0x3,
            scid: ((x >> 6) & 0xff),
            vcid: (x & 0x3f),
            counter: u32::from_be_bytes([0, dat[2], dat[3], dat[4]]),
            replay: (dat[5] >> 7) & 0x1 == 1,
            cycle: (dat[5] >> 6) & 0x1 == 1,
            counter_cycle: dat[5] & 0xf,
        })
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

        let header = VCDUHeader::decode(&dat).unwrap();

        assert_eq!(header.version, 1);
        assert_eq!(header.scid, 85);
        assert_eq!(header.vcid, 33);
        assert_eq!(header.counter, 123_456);
        assert!(!header.replay);
        assert!(!header.cycle);
        assert_eq!(header.counter_cycle, 5);
    }

    #[test]
    fn decode_vcduheader_minmax() {
        let dat: Vec<u8> = vec![0, 0, 0, 0, 0, 0];

        VCDUHeader::decode(&dat).unwrap();

        let dat: Vec<u8> = vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff];

        VCDUHeader::decode(&dat).unwrap();
    }

    #[test]
    fn decode_vcduheader_is_err_when_data_too_short() {
        let zult = VCDUHeader::decode(&[0u8; 0]);
        assert!(zult.is_none());
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
        let frame = Frame::decode(dat).unwrap();

        assert_eq!(frame.data.len(), expected_len);
    }
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct Frame {
    pub header: VCDUHeader,
    /// All frame data bytes, including header
    pub data: Vec<u8>,
}

impl Frame {
    /// Decode ``dat`` into a ``Frame``, or `None` if not enough bytes.
    #[must_use]
    pub fn decode(dat: Vec<u8>) -> Option<Self> {
        let header = VCDUHeader::decode(&dat)?;
        Some(Frame { header, data: dat })
    }

    #[must_use]
    pub fn is_fill(&self) -> bool {
        self.header.vcid == VCID_FILL
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

/// A `Frame` decoded from CADUs containing additional decode information regarding the
/// decoding process, e.g., missing frame counts and Reed-Solomon decoding information,
/// if available.
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub frame: Frame,
    pub missing: u32,
    pub rsstate: RSState,
}

/// Applies Reed-Solomon and PN derandomization to an iterable of Reed-Solomon codeblocks.
///
/// Each block provided must be the full expected length of a codeblock, which includes the
/// length of the frame and the RS parity bytes. All frames are returned without the
/// Reed-Solomon parity bytes.
///
/// RS error detection/correcion occurs in parallel where each codeblock is decoded in full
/// as a single job in a pool of worker threads.
///
/// The missing frame count will be the number of frames mising between consecutive frames
/// for the same VCID.
///
/// Frames are always produced with their RS disposition, even if the data was unable to be
/// corrected. If the RS algorithm fails internally on the given input a error is returned.
///
/// See CCSDS 131.0-b5, section 4.
#[derive(TypedBuilder)]
pub struct FrameRSDecoder {
    /// Reed-Solomon message interleave.
    interleave: u8,
    /// Reed-Solomon algorithm to use. The default is to use RS 223/255 as documented in
    /// the CCSDS blue book.
    #[builder(default=Box::new(DefaultReedSolomon{}))]
    alg: Box<dyn ReedSolomon>,
    /// Number of threads to use for Reed-Solomon.
    #[builder(default)]
    num_threads: usize,
    /// When true, indicates the provided codeblocks are pseudo randomized as described
    /// in 131.0-b5, section 10, and requires derandomization.
    ///
    /// While Pseudo-randomization is not strictly part of Reed-Solomon it is removed at this
    /// stage as a matter of efficiency. To use a custom PN algorithm you can simply disable
    /// this here and apply your own to the codeblocks before using this decoder.
    #[builder(default = true)]
    pseudo_randomized: bool,
}

/// ``FrameDecoder`` is a handle for starting a `DecodedFrameIter`.
impl FrameRSDecoder {
    const DEFAULT_BUFFER_SIZE: usize = 1024;

    /// Starts the decoding processin a the background and returns an iterator for accessing
    /// [Frame]s decoded from the provided blocks.
    ///
    /// # Panics
    /// If the background thread could not be started.
    pub fn decode<B>(self, blocks: B) -> DecodedFrameIter
    where
        B: Iterator<Item = Vec<u8>> + Send + 'static,
    {
        let (jobs_tx, jobs_rx) = bounded(Self::DEFAULT_BUFFER_SIZE);

        let handle = thread::Builder::new()
            .name("frame_rs_decoder".into())
            .spawn(move || {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(self.num_threads)
                    .build()
                    .expect("failed to construct RS threadpool with requested number of threads");

                let jobs_tx = jobs_tx.clone();
                let alg = Arc::new(self.alg);

                let pn = DefaultPN {};

                for mut block in blocks {
                    let (future_tx, future_rx) = unbounded();
                    let alg = Arc::new(alg.clone());

                    // Have to do pn to get the correct header, even for fill
                    if self.pseudo_randomized {
                        block = pn.decode(&block);
                    }
                    let hdr = VCDUHeader::decode(&block).unwrap();
                    // We only do RS on non-fill
                    if hdr.vcid == VCID_FILL {
                        if let Some(frame) = Frame::decode(block) {
                            if future_tx.send(Ok((frame, RSState::NotPerformed))).is_err() {
                                debug!("failed to send frame");
                            }
                        }
                    } else {
                        // spawn_fifo makes sure the frame order is maintained
                        pool.spawn_fifo(move || {
                            let alg = alg.clone();
                            let zult = alg.correct_codeblock(&block, self.interleave).map(
                                |(block, rsstate)| {
                                    (
                                        Frame {
                                            header: hdr,
                                            data: block,
                                        },
                                        rsstate,
                                    )
                                },
                            );
                            if future_tx.send(zult).is_err() {
                                debug!("failed to send frame");
                            }
                        });
                    }
                    // All frames are forwarded, including fill
                    if let Err(err) = jobs_tx.send(future_rx) {
                        debug!("failed to send frame future: {err}");
                    }
                }
            })
            .unwrap();

        DecodedFrameIter {
            done: false,
            jobs: jobs_rx,
            handle: Some(handle),
            last: HashMap::new(),
        }
    }
}

/// Provides [Frame]s based on configuration provided by the parent ``FrameDecoder``.
pub struct DecodedFrameIter {
    done: bool,
    jobs: Receiver<Receiver<Result<(Frame, RSState), IntegrityError>>>,
    handle: Option<JoinHandle<()>>,
    // For tracking missing counts, which are per VCID
    last: HashMap<VCID, u32>,
}

impl Iterator for DecodedFrameIter {
    type Item = Result<DecodedFrame, IntegrityError>;

    fn next(&mut self) -> Option<Self::Item> {
        // recv blocks current thread until data is available.
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
            Ok(rx) => match rx.recv().expect("failed to receive frame future") {
                Ok((frame, rsstate)) => {
                    let span = span!(
                        Level::TRACE,
                        "frame",
                        scid = frame.header.scid,
                        vcid = frame.header.vcid
                    );
                    let _guard = span.enter();
                    // Only compute missing for non-fill frames
                    let missing = if frame.header.vcid == VCID_FILL {
                        0
                    } else if let Some(last) = self.last.get(&frame.header.vcid) {
                        missing_frames(frame.header.counter, *last)
                    } else {
                        self.last.insert(frame.header.vcid, frame.header.counter);
                        0
                    };
                    self.last.insert(frame.header.vcid, frame.header.counter);

                    Some(Ok(DecodedFrame {
                        frame,
                        missing,
                        rsstate,
                    }))
                }
                Err(err) => Some(Err(err)),
            },
        }
    }
}

/// Simpler version of ``FrameRSDecoder``, but for streams that do not use Reed-Solomon.
///
/// The `rsstate` property of returned `DecodedFrames` will always be ``RSState::NotPerformed``.
///
/// Each block in `blocks` should have the expected length of a frame.
#[derive(TypedBuilder)]
pub struct FrameDecoder {
    #[builder(default = true)]
    pseudo_randomized: bool,
}

impl FrameDecoder {
    /// Start the deocde process in the background and return an interator that will
    /// make decoded frames available as they are available.
    ///
    /// # Panics
    /// If there are not enough bytes to decode a frame.
    pub fn decode<B>(self, blocks: B) -> impl Iterator<Item = DecodedFrame> + Send + 'static
    where
        B: Iterator<Item = Vec<u8>> + Send + 'static,
    {
        let pn = DefaultPN {};
        let mut last: HashMap<VCID, u32> = HashMap::default();

        let frames = blocks.map(move |mut block| {
            if self.pseudo_randomized {
                block = pn.decode(&block);
            }
            let frame = Frame::decode(block).expect("Failed to create from from block; too short?");
            // Only compute missing for non-fill frames
            let missing = if frame.header.vcid == VCID_FILL {
                0
            } else if let Some(last) = last.get(&frame.header.vcid) {
                missing_frames(frame.header.counter, *last)
            } else {
                last.insert(frame.header.vcid, frame.header.counter);
                0
            };
            last.insert(frame.header.vcid, frame.header.counter);

            DecodedFrame {
                frame,
                missing,
                rsstate: RSState::NotPerformed,
            }
        });

        frames
        // DecodedFrameIter2 {
        //     frames: Box::new(frames),
        // }
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
mod tests {
    use super::*;
    use crate::{Synchronizer, ASM};
    use std::{fs, path::PathBuf};

    fn fixture_path(name: &str) -> PathBuf {
        let mut path = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        dbg!(&path);
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

        let frame = Frame::decode(dat).unwrap();
        assert_eq!(frame.header.scid, 157);
        assert_eq!(frame.header.vcid, 16);

        let mpdu = frame.mpdu(0, 0).unwrap();
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
        let reader = fs::File::open(&fpath).unwrap_or_else(|_| panic!("{fpath:?} to exist"));
        let blocks = Synchronizer::new(reader, &ASM, 1020)
            .into_iter()
            .filter_map(std::io::Result::ok);

        let frames: Vec<Result<DecodedFrame, IntegrityError>> = FrameRSDecoder::builder()
            .interleave(4)
            .build()
            .decode(blocks)
            .collect();

        assert_eq!(frames.len(), 7, "expected frame count doesn't match");
        for (idx, df) in frames.into_iter().enumerate() {
            let df = df.unwrap();
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
