use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::pn::{DefaultPN, PNDecoder};
use crate::rs::{DefaultReedSolomon, IntegrityError, RSState, ReedSolomon};
use crossbeam::channel::{bounded, unbounded, Receiver};
use serde::{Deserialize, Serialize};
use tracing::{debug, span, trace, Level};

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

#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub frame: Frame,
    pub missing: u32,
    pub rsstate: RSState,
}

pub struct FrameDecoder<R, P>
where
    R: ReedSolomon,
    P: PNDecoder,
{
    interleave: u8,
    buffer_size: usize,

    pn_decoder: Option<P>,
    reed_solomon: Option<R>,
    reed_solomon_threads: usize,
    reed_solomon_skip_vcids: HashSet<VCID>,
}

/// ``FrameDecoder`` is a handle for starting a `DecodedFrameIter`.
impl<R, P> FrameDecoder<R, P>
where
    R: ReedSolomon + 'static,
    P: PNDecoder + 'static,
{
    /// Start decoding in the background and return an iterator for retrieving decoded frames.
    ///
    /// # Panics
    /// If the background thread could not be started.
    pub fn start<B>(self, blocks: B) -> DecodedFrameIter
    where
        B: Iterator<Item = Vec<u8>> + Send + 'static,
    {
        // A "job" in this context is the processing of 1 block. Receivers on which the
        // RS results are delivered as sent on this channel, one for each block.
        let (jobs_tx, jobs_rx) = bounded(self.buffer_size);

        let interleave = self.interleave;

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
                let reed_solomon = Arc::new(self.reed_solomon);
                let pn_decoder = Arc::new(self.pn_decoder);
                let reed_solomon_skip_vcids = self.reed_solomon_skip_vcids.clone();

                for mut block in blocks {
                    let reed_solomon = reed_solomon.clone();
                    let reed_solomon_skip_vcids = reed_solomon_skip_vcids.clone();
                    let pn_decoder = pn_decoder.clone();
                    let (future_tx, future_rx) = unbounded();
                    // spawn_fifo makes sure the frame order is maintained
                    pool.spawn_fifo(move || {
                        // Only do PN if not None
                        if let Some(pn) = pn_decoder.borrow() {
                            block = pn.decode(&block);
                        }

                        let zult = match reed_solomon.borrow() {
                            Some(rs) => {
                                // Don't do RS on fill VCIDs
                                // Blocks will never be short, so unwrap
                                let vcid = VCDUHeader::decode(&block).unwrap().vcid;
                                if reed_solomon_skip_vcids.contains(&vcid) {
                                    Ok((block, RSState::NotPerformed))
                                } else {
                                    rs.correct_codeblock(&block, interleave)
                                }
                            }
                            None => Ok((block, RSState::NotPerformed)),
                        };

                        let zult = future_tx.send(zult.map(|(block, state)| {
                            // block should always contain the minimum bytes for a frame
                            let frame = Frame::decode(block).expect("failed to decode frame");
                            (frame, state)
                        }));

                        if zult.is_err() {
                            debug!("failed to send frame");
                        }
                    });

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

/// Provides [Frame]s based on configuration provided by the parent ``FrameDecoderBuilder``.
pub struct DecodedFrameIter {
    done: bool,
    jobs: Receiver<Receiver<Result<(Frame, RSState), IntegrityError>>>,
    handle: Option<JoinHandle<()>>,
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
                        let missing = missing_frames(frame.header.counter, *last);
                        if missing > 0 {
                            trace!(
                                cur = frame.header.counter,
                                last = last,
                                missing = missing,
                                "missing frames",
                            );
                        }
                        missing
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
pub struct FrameDecoderBuilder<R, P>
where
    R: ReedSolomon,
    P: PNDecoder,
{
    interleave: u8,
    buffer_size: usize,

    pn_decoder: Option<P>,
    reed_solomon: Option<R>,
    reed_solomon_threads: usize,
    reed_solomon_skip_vcids: HashSet<VCID>,
}

impl<R, P> FrameDecoderBuilder<R, P>
where
    R: ReedSolomon,
    P: PNDecoder,
{
    /// Default number of frames to buffer in memory while waiting for RS.
    pub const DEFAULT_BUFFER_SIZE: usize = 1024;

    /// Limits the number of block waiting in memory for RS.
    /// See ``FrameDecoderBuilder::DEFAULT_BUFFER_SIZE``.
    #[must_use]
    pub fn buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Set VCIDs to skip when performing RS.
    ///
    /// The default is to skip only ``VCID_FILL``.
    ///
    /// If you explicitly set the vcids to skip you will need to include `VCID_FILL`.
    #[must_use]
    pub fn reed_solomon_skip_vcids(mut self, vcids: &[VCID]) -> Self {
        self.reed_solomon_skip_vcids.clear();
        self.reed_solomon_skip_vcids.extend(vcids.iter());
        self
    }

    /// Set the number of threads to use for Reed-Solomon. If not explicitly set, the
    /// number of threads is chosen automatically.
    #[must_use]
    pub fn reed_solomon_threads(mut self, num: usize) -> Self {
        self.reed_solomon_threads = num;
        self
    }

    /// Set pseudo-noise implementation.
    #[must_use]
    pub fn pn_decode(mut self, pn: Option<P>) -> Self {
        self.pn_decoder = pn;
        self
    }

    /// Build the `FrameDecoder`.
    #[must_use]
    pub fn build(self) -> FrameDecoder<R, P> {
        FrameDecoder {
            interleave: self.interleave,
            buffer_size: self.buffer_size,
            pn_decoder: self.pn_decoder,
            reed_solomon: self.reed_solomon,
            reed_solomon_threads: self.reed_solomon_threads,
            reed_solomon_skip_vcids: self.reed_solomon_skip_vcids,
        }
    }
}

impl<R> Default for FrameDecoderBuilder<R, DefaultPN>
where
    R: ReedSolomon,
{
    /// Creates a builder configured with some sensible defaults.
    fn default() -> FrameDecoderBuilder<R, DefaultPN> {
        let mut skip_vcids: HashSet<VCID> = HashSet::new();
        skip_vcids.insert(VCID_FILL);

        FrameDecoderBuilder {
            interleave: 0,
            pn_decoder: Some(DefaultPN),
            reed_solomon: None,
            reed_solomon_threads: 0, // Let rayon decide
            reed_solomon_skip_vcids: skip_vcids,
            buffer_size: Self::DEFAULT_BUFFER_SIZE,
        }
    }
}

impl<P> FrameDecoderBuilder<DefaultReedSolomon, P>
where
    P: PNDecoder,
{
    /// Use the default Reed-Solomon 223/255 with the specified interleave value.
    ///
    /// # Panics
    /// If `interleave` is 0.
    #[must_use]
    pub fn reed_solomon(mut self, interleave: u8) -> Self {
        self.reed_solomon = Some(DefaultReedSolomon {});
        self.interleave = interleave;
        self
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

/// Get a ``spacecrafts::FramingConfig`` for a spacecraft, or `None`.
///
/// Attempt to load the DB from `path`, if provided, otherwise look for a locally available
/// [spacecraftsdb](https://github.com/bmflynn/spacecraftsdb) file available at one of the
/// default locations.
///
/// # Errors
/// If the spacecraftdb database file is not found in one of the standard locations.
pub fn framing_config(
    scid: SCID,
    path: Option<&str>,
) -> Result<Option<spacecrafts::FramingConfig>, Box<dyn std::error::Error>> {
    let db = match path {
        Some(path) => spacecrafts::DB::with_path(path)?,
        None => spacecrafts::DB::new()?,
    };

    Ok(match db.find(scid) {
        Some(sc) => Some(sc.framing_config),
        None => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Synchronizer, ASM};
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
        let reader = fs::File::open(fpath).unwrap();
        let blocks = Synchronizer::new(reader, &ASM.to_vec(), 1020)
            .into_iter()
            .filter_map(std::io::Result::ok);

        let frames: Vec<Result<DecodedFrame, IntegrityError>> = FrameDecoderBuilder::default()
            .reed_solomon(4)
            .build()
            .start(blocks)
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
