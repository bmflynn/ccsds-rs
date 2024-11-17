use std::{
    borrow::Borrow,
    collections::HashMap,
    sync::Arc,
    thread::{self, JoinHandle},
};

use super::{
    missing_frames, DefaultDerandomizer, DefaultReedSolomon, Derandomizer, Frame, Integrity,
    IntegrityAlgorithm, VCDUHeader,
};
use crate::prelude::*;
use crossbeam::channel::{bounded, unbounded, Receiver};
use tracing::{debug, span, Level};

/// Decodes CADU bytes into [Frame]s.
///
/// # Examples
/// Default decode using default CCSDS derandomization and reed-solomon.
/// ```no_run
/// use ccsds::framing::Decoder;
///
/// const cadu_len: usize = 1020;
/// let reed_solomon_interleave = 4;
/// let blocks: Vec<Vec<u8>> = vec![
///   vec![0u8; cadu_len],
/// ];
/// let frames = Decoder::default_ccsds(reed_solomon_interleave)
///     .decode(blocks.into_iter())
///     .filter_map(Result::ok);
/// ```
/// Manually specified decode using default CCSDS derandomization and reed-solomon.
/// ```no_run
/// use ccsds::framing::{Decoder, DefaultReedSolomon, DefaultDerandomizer};
/// const cadu_len: usize = 1020;
/// let reed_solomon_interleave = 4;
/// let blocks: Vec<Vec<u8>> = vec![
///   vec![0u8; cadu_len],
/// ];
/// let frames = Decoder::default_ccsds(reed_solomon_interleave)
///     .with_integrity(Box::new(DefaultReedSolomon::new(reed_solomon_interleave)))
///     .with_derandomization(Box::new(DefaultDerandomizer))
///     .decode(blocks.into_iter())
///     .filter_map(Result::ok);
/// ```
#[derive(Default)]
pub struct Decoder {
    num_threads: Option<u32>,
    derandomization: Option<Box<dyn Derandomizer>>,
    integrity: Option<Box<dyn IntegrityAlgorithm>>,
}

impl Decoder {
    const DEFAULT_BUFFER_SIZE: usize = 1024;

    pub fn new() -> Self {
        Decoder {
            num_threads: None,
            derandomization: None,
            integrity: None,
        }
    }

    pub fn default_ccsds(interleave: u8) -> Self {
        Self {
            num_threads: None,
            derandomization: Some(Box::new(DefaultDerandomizer)),
            integrity: Some(Box::new(DefaultReedSolomon::new(interleave))),
        }
    }

    pub fn with_derandomization(mut self, derandomizer: Box<dyn Derandomizer>) -> Self {
        self.derandomization = Some(derandomizer);
        self
    }

    pub fn with_integrity(mut self, integrity: Box<dyn IntegrityAlgorithm>) -> Self {
        self.integrity = Some(integrity);
        self
    }

    pub fn with_integrity_threads(mut self, num: u32) -> Self {
        self.num_threads = Some(num);
        self
    }

    /// Returns an interator that performs the decode, including derandomization and integrity
    /// checks, if configured.
    ///
    /// The returned iterator performs all decoding in a background thread. Integrity checking is
    /// further performed
    ///
    /// # Errors
    /// [Error] if integrity checking is used and fails.
    pub fn decode<B>(self, cadus: B) -> impl Iterator<Item = Result<DecodedFrame>>
    where
        B: Iterator<Item = Vec<u8>> + Send + 'static,
    {
        let (jobs_tx, jobs_rx) = bounded(Self::DEFAULT_BUFFER_SIZE);

        let handle = thread::Builder::new()
            .name("frame_rs_decoder".into())
            .spawn(move || {
                let pool = {
                    let mut pool = rayon::ThreadPoolBuilder::new();
                    if let Some(num) = self.num_threads {
                        pool = pool.num_threads(num as usize);
                    }
                    pool
                }
                .build()
                .expect("failed to construct RS threadpool with requested number of threads");

                let jobs_tx = jobs_tx.clone();
                let integrity_alg = Arc::new(self.integrity);

                for (idx, mut block) in cadus.enumerate() {
                    let (future_tx, future_rx) = unbounded();

                    let integrity_alg = integrity_alg.clone();

                    // Have to do pn to get the correct header, even for fill
                    // FIXME: Is it? Randomized fill VCID should be a static value, right?
                    if let Some(ref pn) = self.derandomization {
                        block = pn.derandomize(&block);
                    }

                    let Some(hdr) = VCDUHeader::decode(&block) else {
                        debug!(block_idx = idx, "cannot decode header; skipping");
                        continue;
                    };
                    // We only do RS on non-fill
                    if hdr.vcid == VCDUHeader::FILL {
                        if let Some(frame) = Frame::decode(block) {
                            if future_tx
                                .send(Ok(DecodedFrame {
                                    frame,
                                    missing: 0,
                                    integrity: None,
                                }))
                                .is_err()
                            {
                                debug!(block_idx = idx, "failed to send fill frame");
                            }
                        }
                    } else {
                        // spawn_fifo makes sure the frame order is maintained
                        pool.spawn_fifo(move || {
                            let zult = if let Some(integrity_alg) = integrity_alg.clone().borrow() {
                                match integrity_alg.perform(&block) {
                                    Ok((status, data)) => Ok(DecodedFrame {
                                        frame: Frame { header: hdr, data },
                                        missing: 0,
                                        integrity: Some(status),
                                    }),
                                    Err(err) => Err(err),
                                }
                            } else {
                                Ok(DecodedFrame {
                                    frame: Frame {
                                        header: hdr,
                                        data: block,
                                    },
                                    missing: 0,
                                    integrity: None,
                                })
                            };

                            if future_tx.send(zult).is_err() {
                                debug!(block_idx = idx, "failed to send frame");
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

/// A [Frame] decoded from CADUs containing additional decode information regarding the
/// decoding process, e.g., missing frame counts and Reed-Solomon decoding information,
/// if available.
#[derive(Debug)]
pub struct DecodedFrame {
    pub frame: super::Frame,
    pub missing: u32,
    pub integrity: Option<Integrity>,
}

/// Provides [Frame]s based on configuration provided by the parent ``FrameDecoder``.
struct DecodedFrameIter {
    done: bool,
    jobs: Receiver<Receiver<Result<DecodedFrame>>>,
    handle: Option<JoinHandle<()>>,
    // For tracking missing counts, which are per VCID
    last: HashMap<super::Vcid, u32>,
}

impl Iterator for DecodedFrameIter {
    type Item = Result<DecodedFrame>;

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
                Ok(mut decoded_frame) => {
                    let frame = &decoded_frame.frame;
                    let span = span!(
                        Level::TRACE,
                        "frame",
                        scid = frame.header.scid,
                        vcid = frame.header.vcid
                    );
                    let _guard = span.enter();

                    // Only compute missing for non-fill frames
                    decoded_frame.missing = if frame.header.vcid == VCDUHeader::FILL {
                        0
                    } else if let Some(last) = self.last.get(&frame.header.vcid) {
                        missing_frames(frame.header.counter, *last)
                    } else {
                        self.last.insert(frame.header.vcid, frame.header.counter);
                        0
                    };
                    self.last.insert(frame.header.vcid, frame.header.counter);

                    Some(Ok(decoded_frame))
                }
                Err(err) => Some(Err(err)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use crate::framing::{Synchronizer, ASM, MPDU};

    use super::*;

    fn fixture_path(name: &str) -> PathBuf {
        let mut path = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        dbg!(&path);
        path.push(name);
        path
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
        let blocks: Vec<Vec<u8>> = Synchronizer::new(reader, &ASM, 1020)
            .into_iter()
            .map(|a| a.unwrap())
            .collect();
        assert_eq!(blocks.len(), 7);

        let frames: Vec<Result<DecodedFrame>> = Decoder::default_ccsds(4)
            .decode(blocks.into_iter())
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
}
