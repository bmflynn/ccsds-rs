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
/// use ccsds::framing::decode_frames_rs;
///
/// const cadu_len: usize = 1020;
/// let reed_solomon_interleave = 4;
/// let blocks: Vec<Vec<u8>> = vec![
///   vec![0u8; cadu_len],
/// ];
/// let frames = decode_frames_rs(blocks.into_iter(), reed_solomon_interleave)
///     .filter_map(Result::ok);
/// ```
/// Manually specified decode using default CCSDS derandomization and reed-solomon.
/// ```no_run
/// use ccsds::framing::{FrameDecoder, DefaultReedSolomon, DefaultDerandomizer};
/// const cadu_len: usize = 1020;
/// let reed_solomon_interleave = 4;
/// let blocks: Vec<Vec<u8>> = vec![
///   vec![0u8; cadu_len],
/// ];
/// let frames = FrameDecoder::new()
///     .with_integrity(Box::new(DefaultReedSolomon::new(reed_solomon_interleave)))
///     .with_derandomization(Box::new(DefaultDerandomizer))
///     .decode(blocks.into_iter())
///     .filter_map(Result::ok);
/// ```
#[derive(Default)]
pub struct FrameDecoder {
    num_threads: Option<u32>,
    derandomization: Option<Box<dyn Derandomizer>>,
    integrity: Option<Box<dyn IntegrityAlgorithm>>,
    integrity_noop: bool,
}

impl FrameDecoder {
    const DEFAULT_BUFFER_SIZE: usize = 1024;

    pub fn new() -> Self {
        FrameDecoder::default()
    }

    /// Apply derandomization using the provided algorithm. If not provided no derandomization is
    /// performed.
    pub fn with_derandomization(mut self, derandomizer: Box<dyn Derandomizer>) -> Self {
        self.derandomization = Some(derandomizer);
        self
    }

    /// Perform integrity checking with the give algorithm. If not provided, no configuration
    /// checking is performed.
    pub fn with_integrity(mut self, integrity: Box<dyn IntegrityAlgorithm>) -> Self {
        self.integrity = Some(integrity);
        self
    }

    /// Do not perform integrity check. Useful when there are parity bytes to remove but you do not
    /// want to perform the algorithm.
    pub fn with_integrity_noop(mut self) -> Self {
        self.integrity_noop = true;
        self
    }

    /// Use this number of threads for integrity checks. By default the number of threads is
    /// configured automatically and is typically the number of CPUs available on the system.
    pub fn with_integrity_threads(mut self, num: u32) -> Self {
        self.num_threads = Some(num);
        self
    }

    /// Returns an interator that performs the decode, including derandomization and integrity
    /// checks, if configured.
    ///
    /// Integrity checks are not performed on VCDU fill frames (vcid=63), however, fill frames are
    /// not filtered and are produced by the returned iterator.
    ///
    /// Integrity checking is handled in parallel with a distinct job per-CADU using an
    /// automatically configured number of threads by default, otherwise the number of threads
    /// set using [Self::with_integrity_threads].
    ///
    /// # Errors
    /// [Error] if integrity checking is used and fails.
    pub fn decode<B>(self, cadus: B) -> impl Iterator<Item = Result<DecodedFrame>>
    where
        B: Iterator<Item = Vec<u8>> + Send + 'static,
    {
        let (jobs_tx, jobs_rx) = bounded(Self::DEFAULT_BUFFER_SIZE);

        let handle = thread::spawn(move || {
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
                if let Some(ref pn) = self.derandomization {
                    block = pn.derandomize(&block).to_vec();
                }

                let Some(hdr) = VCDUHeader::decode(&block) else {
                    debug!(block_idx = idx, "cannot decode header; skipping");
                    continue;
                };

                // No integrity checking on FILL, however, we still remove parity bytes
                if hdr.vcid == VCDUHeader::FILL || self.integrity_noop {
                    let data = match integrity_alg.clone().borrow() {
                        Some(alg) => alg.remove_parity(&block),
                        None => &block,
                    };

                    if let Some(frame) = Frame::decode(data.to_vec()) {
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
                    // Do integrity checking in the thread pool. Use spawn_fifo to make sure the frame
                    // order is maintained.
                    pool.spawn_fifo(move || {
                        let decoded_frame =
                            if let Some(integrity_alg) = integrity_alg.clone().borrow() {
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

                        if future_tx.send(decoded_frame).is_err() {
                            debug!(block_idx = idx, "failed to send frame");
                        }
                    });
                }
                // All frames are forwarded, including fill
                if let Err(err) = jobs_tx.send(future_rx) {
                    debug!("failed to send frame future: {err}");
                }
            }
        });

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
#[derive(Debug, Clone)]
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

/// Decodes CADU bytes into [Frame]s.
///
/// `cadus` must provide `Vec<u8>` data of the length required by the provided integrity algorithm.
/// For example, [DefaultReedSolomon] requires parity bytes that are not strictly part of the frame data
/// and will require a CADU length of `255 * rs_interleave` and will result in an output
/// [DecodedFrame] data length of `255 * rs_interleave - (rs_num_correctable * interleave)` bytes.
///
/// Other integrity algorithms, e.g., Crc32, may not require parity bytes and will have the same
/// length frame data and CADU length.
///
/// Also note, the input `cadus` must not include any attached sync marker bytes.
///
/// # Examples
/// ```no_run
/// use ccsds::framing::{decode_frames, DefaultReedSolomon, DefaultDerandomizer};
/// const cadu_len: usize = 1020;
/// let reed_solomon_interleave = 4;
/// let cadus: Vec<Vec<u8>> = vec![
///   vec![0u8; cadu_len],
/// ];
/// let frames = decode_frames(
///     cadus.into_iter(),
///     Some(Box::new(DefaultReedSolomon::new(reed_solomon_interleave))),
///     Some(Box::new(DefaultDerandomizer)),
/// ).filter_map(Result::ok);
/// ```
pub fn decode_frames<I>(
    cadus: I,
    integrity: Option<Box<dyn IntegrityAlgorithm>>,
    pn: Option<Box<dyn Derandomizer>>,
) -> impl Iterator<Item = Result<DecodedFrame>>
where
    I: Iterator<Item = Vec<u8>> + Send + 'static,
{
    let mut decoder = FrameDecoder::new();
    if let Some(pn) = pn {
        decoder = decoder.with_derandomization(pn);
    }
    if let Some(integrity) = integrity {
        decoder = decoder.with_integrity(integrity);
    }
    decoder.decode(cadus)
}

/// Wraps [decode_frames] providing standard CCSDS Reed-Solomon(223/255)and the default CCSDS
/// derandomization appropriate for most spacecraft that use RS.
///
/// See [decode_frames].
///
/// # Examples
/// ```no_run
/// use ccsds::framing::decode_frames_rs;
/// const cadu_len: usize = 1020;
/// let reed_solomon_interleave = 4;
/// let cadus: Vec<Vec<u8>> = vec![
///   vec![0u8; cadu_len],
/// ];
/// let frames = decode_frames_rs(cadus.into_iter(), reed_solomon_interleave)
///     .filter_map(Result::ok);
/// ```
pub fn decode_frames_rs<I>(cadus: I, interleave: u8) -> impl Iterator<Item = Result<DecodedFrame>>
where
    I: Iterator<Item = Vec<u8>> + Send + 'static,
{
    decode_frames(
        cadus,
        Some(Box::new(DefaultReedSolomon::new(interleave))),
        Some(Box::new(DefaultDerandomizer)),
    )
}

/*
/// Wraps [decode_frames] providing standard CCSDS crc32 and the default CCSDS derandomization
/// appropriate for most spacecraft that use CRSs.
///
/// # Examples
/// ```no_run
/// use ccsds::framing::decode_frames_rs;
/// const cadu_len: usize = 1020;
/// let offset = 1016;
/// let cadus: Vec<Vec<u8>> = vec![
///   vec![0u8; cadu_len],
/// ];
/// let frames = decode_frames_crc32(cadus.into_iter(), offset)
///     .filter_map(Result::ok);
/// ```
pub fn decode_frames_crc32<I>(
    cadus: I,
    offset: usize,
) -> impl Iterator<Item = Result<DecodedFrame>>
where
    I: Iterator<Item = Vec<u8>> + Send + 'static,
{
    decode_frames(
        cadus,
        Some(Box::new(DefaultCrc32::new(offset))),
        Some(Box::new(DefaultDerandomizer)),
    )
}
*/

#[cfg(test)]
mod tests {
    use crate::framing::MPDU;

    use super::*;

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
}
