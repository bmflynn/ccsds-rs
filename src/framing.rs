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
    pub interleave: i32,
    pub correctable: i32,
    pub vfill_length: i32,
}

/// Spacecraft framing configuration.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct Framing {
    pub asm: Vec<u8>,
    /// Length of the frame contained within a CADU, not including the ASM or
    /// any Reed-solomon parity bytes.
    ///
    /// This length, along with the [Self::izone_length] and [Self::trailer_length] will effectively
    /// define the length of an MPDU.
    pub frame_length: i32,
    pub pseudo_randomized: bool,
    pub izone_length: i32,
    pub trailer_length: i32,
    pub rs: Option<RSConfig>,
}

impl Framing {
    /// Returns the expected length of CADU which will include the ASM and the
    /// length of the Reed-Solomon code block.
    ///
    /// So, for example, with standard RS(223/255) with an interleave of 4 this
    /// will return 1024, which is 4 bytes for the ASM, 128 bits for the Reed-Solomon
    /// code block and the frame bytes.
    pub fn cadu_len(&self) -> i32 {
        let rslen = match self.rs {
            Some(ref rs) => (rs.correctable * 2) * rs.interleave,
            None => 0,
        };
        self.asm.len() as i32 + self.frame_length + rslen
    }

    /// Return the length of a MPDU. This will be the [Self::frame_length] minus any bytes
    /// for the insert zone or trailer.
    pub fn mpdu_len(&self) -> i32 {
        self.frame_length - self.izone_length - self.trailer_length
    }
}

pub const VCID_FILL: VCID = 63;
pub const MPDU_FILL: u16 = 0x7fe;
pub const MPDU_NO_HEADER: u16 = 0x7ff;

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
    pub const LEN: usize = 6;
    pub const COUNTER_MAX: u32 = 0xffffff - 1;

    pub fn decode(dat: &Vec<u8>) -> Self {
        if dat.len() < Self::LEN as usize {
            panic!(
                "vcdu header requires {} bytes, got {}",
                Self::LEN,
                dat.len()
            );
        }
        let x = u16::from_be_bytes([dat[0], dat[1]]);
        VCDUHeader {
            version: (dat[0] >> 6) & 0x3,
            scid: ((x >> 6) & 0xff).into(),
            vcid: (x & 0x3f).into(),
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
        assert_eq!(header.counter, 123456);
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

#[derive(Debug)]
pub struct Frame {
    pub header: VCDUHeader,
    /// All frame data bytes, including header
    pub data: Vec<u8>,
}

impl Frame {
    pub fn decode(dat: Vec<u8>) -> Self {
        let header = VCDUHeader::decode(&dat);
        Frame { header, data: dat }
    }

    pub fn mpdu(&self, izone_length: i32, trailer_length: i32) -> Vec<u8> {
        let start: usize = VCDUHeader::LEN + izone_length as usize;
        let end: usize = start + self.data.len() - trailer_length as usize;

        self.data[start..end].to_vec()
    }
}

pub struct DecodedFrameIter {
    done: bool,
    jobs: Receiver<Receiver<(Frame, RSState)>>,
    errch: Receiver<std::io::Error>,
    handle: Option<JoinHandle<()>>,
}

impl DecodedFrameIter {
    // Return the error that caused decoding to fail early, or None. This will always
    // return [None] if the iterator is not finished.
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
    type Item = (Frame, RSState);

    fn next(&mut self) -> Option<Self::Item> {
        return match self.jobs.recv() {
            Err(_) => {
                self.done = true;
                self.handle
                    .take()
                    .expect("bad state, handle should not be None")
                    .join()
                    .expect("reedsolomon thread paniced");
                None
            }
            Ok(rx) => Some(rx.recv().expect("failed to receive future")),
        };
    }
}

/// Builds a [DecodedFrameIter] that will return all frames decoded from the stream read
/// from reader.
///
/// Reads are only performed when a [Frame] is requested from the returned iterator, i.e.,
/// when [Iteator.next] is called. More bytes than the size of the frame may be read if the
/// underlying stream is not synchronized.
///
/// Frames will generated in the order in which they occur in the original byte stream.
///
/// IO is performed concurrently so the iterator can be returned immediately. All PN
/// and RS decoding is likewise performed concurrently.
pub struct FrameDecoderBuilder {
    asm: Vec<u8>,
    interleave: i32,
    cadu_length: i32,
    buffer_size: usize,

    pn_decoder: Option<PNDecoder>,
    reed_solomon: Option<Box<dyn ReedSolomon + Sync + 'static>>,
    reed_solomon_threads: usize,
}

impl FrameDecoderBuilder {
    /// Default number of frames to buffer in memory while waiting for RS.
    pub const DEFAULT_BUFFER_SIZE: usize = 1024;

    /// Create a new [DecodedFrameIter] with default values. For most cases all that
    /// should be necessary is the following:
    /// ```
    /// use ccsds::FrameDecoderBuilder;
    /// let r = &[0u8; 1][..]; // implements Read
    /// let builder = FrameDecoderBuilder::new(1024, 4);
    /// builder.build(r);
    /// ```
    ///
    /// It is possible, however, to twidle with default implementations using the provided
    /// builder functions.
    pub fn new(cadu_length: i32, interleave: i32) -> Self {
        FrameDecoderBuilder {
            cadu_length,
            interleave,
            asm: ASM.to_vec(),
            pn_decoder: Some(pn_decode),
            reed_solomon: Some(Box::new(DefaultReedSolomon {})),
            reed_solomon_threads: 0, // Let rayon decide
            buffer_size: Self::DEFAULT_BUFFER_SIZE,
        }
    }

    /// Limits the number of block waiting in memory for RS.
    /// See [FrameDecoderBuilder::DEFAULT_BUFFER_SIZE].
    pub fn buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Set the CADU Attached Sync Marker used to synchronize the incoming stream.
    /// Defaults to [ASM];
    pub fn attached_sync_marker(mut self, asm: &[u8]) -> Self {
        self.asm = asm.to_vec();
        self
    }

    /// Set the Reed-solomon per-CADU implementation to use.
    /// Defaults to [DefaultReedSolomon::correct_codeblock].
    pub fn reed_solomon(mut self, rs: Box<dyn ReedSolomon + Sync>) -> Self {
        self.reed_solomon = Some(rs);
        self
    }

    /// Set the number of threads to use for Reed-Solomon.
    /// Defaults to     
    pub fn reed_solomon_threads(mut self, num: usize) -> Self {
        self.reed_solomon_threads = num;
        self
    }

    pub fn pn_decode(mut self, pn: PNDecoder) -> Self {
        self.pn_decoder = Some(pn);
        self
    }

    /// Returns a [DecodedFrameIter] configured according to the provided options.
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
                    Synchronizer::new(reader, &self.asm, self.cadu_length - self.asm.len() as i32);
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
                        if let Some(pn_decode) = pn_decoder {
                            pn_decode(&mut block);
                        }
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
        }
    }
}
