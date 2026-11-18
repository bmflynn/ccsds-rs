use std::sync::Arc;
use std::{collections::HashMap, io::Read};

use crossbeam::channel::Sender;
use tracing::debug;

use crate::framing::synchronizer::Synchronizer;
use crate::framing::{
    missing_frames,
    reed_solomon::{RsBackend, RsData, RsOpts},
    DefaultDerandomizer, DefaultReedSolomon, Derandomizer, Frame, ReedSolomon, VCDUHeader,
};
use crate::framing::{Block, Counter, Integrity, PacketDemux, ASM};
use crate::spacepacket::Packet;
use crate::Result;

/// Perform derandomization on each input [Block] using [DefaultDerandomizer].
pub fn derandomize<I>(cadus: I) -> impl Iterator<Item = Vec<u8>>
where
    I: Iterator<Item = Vec<u8>>,
{
    let pn = DefaultDerandomizer;

    cadus.map(move |mut cadu| {
        pn.derandomize_inplace(&mut cadu);
        cadu
    })
}

/// Options used for synchronization
pub struct SyncOpts {
    asm: Vec<u8>,
    length: usize,
}

impl SyncOpts {
    /// Create a new set of sync options.
    ///
    /// # Arguments
    /// * `length` Length of data to return as a [crate::framing::Block], not including the length of the attached
    ///   sync marker.
    pub fn new(length: usize) -> Self {
        SyncOpts {
            asm: ASM.to_vec(),
            length,
        }
    }

    /// Attached sync marker indicating the start of a [crate::framing::Block].
    pub fn with_asm(mut self, asm: &[u8]) -> Self {
        self.asm = asm.to_vec();
        self
    }
}

/// Syncronize a bit stream to provide a byte-aligned iterator of [crate::framing::Block] data.
pub fn synchronize<R>(reader: R, opts: SyncOpts) -> impl Iterator<Item = Result<Block>>
where
    R: Read + Send + 'static,
{
    // sync_on_thread(reader, block_length, 10)
    let sync = Synchronizer::new(reader, opts.length).with_asm(&opts.asm);

    sync.into_iter()
}

/// Decode CADUs from `reader` according to the provided [SyncOpts]. If `pn` apply
/// derandomization and if `rs` also perform default CCSDS Reed-Solomon. The CADU
/// payload is not decoded to frames, so this is appropriate for decoding any type
/// of CADU payload that uses standard CCSDS PN and RS.
///
/// If no RS is performed the integrity will always be `None`.
///
/// Any error reading from `reader` will terminate decoding.
pub fn decode_cadus<R>(
    reader: R,
    sync: SyncOpts,
    pn: bool,
    rs: Option<RsOpts>,
) -> impl Iterator<Item = (Vec<u8>, Option<Integrity>)>
where
    R: Read + Send + 'static,
{
    let mut cadus: Box<dyn Iterator<Item = Vec<u8>> + Send> = Box::new(
        synchronize(reader, sync)
            .map_while(Result::ok)
            .map(|b| b.data),
    );

    if pn {
        cadus = Box::new(derandomize(cadus));
    }

    let rs_cadus: Box<dyn Iterator<Item = (Vec<u8>, Option<Integrity>)> + Send> =
        if let Some(opts) = rs {
            Box::new(reed_solomon(cadus, opts).map(|r| (r.data, Some(r.integrity))))
        } else {
            Box::new(cadus.map(|data| (data, None)))
        };
    rs_cadus
}

/// Decode input `Vec<u8>` data into [Frame] data updating [Frame::state] with the current
/// state.
///
/// Frames produced will always have a non-`None` [Frame::state].
pub fn frame_decoder<I>(payloads: I) -> impl Iterator<Item = Frame> + Send + 'static
where
    I: Iterator<Item = Vec<u8>> + Send + 'static,
{
    let mut counters: HashMap<u16, u32> = HashMap::default();
    payloads.filter_map(move |data| {
        let mut frame = Frame::decode(&data);
        let header = &frame.header;
        let mut missing = 0;
        let mut duplicate = false;
        if VCDUHeader::FILL != header.vcid {
            if let Some(last) = counters.get(&header.vcid) {
                duplicate = header.counter == *last;
                missing = missing_frames(header.counter, *last, header.counter_max());
            }
        }
        frame.state = Some(Counter { missing, duplicate });
        counters.insert(header.vcid, header.counter);
        Some(frame)
    })
}

fn do_reed_solomon<I>(frames: I, opts: RsOpts, result_tx: Sender<RsData>)
where
    I: Iterator<Item = Vec<u8>> + Send + 'static,
{
    // Thread pool to host the  RS computation tasks. 1 job per frame, which results in
    // `interleave` computations per frame as a single job.
    let pool = rayon::ThreadPoolBuilder::new()
        .thread_name(|i| format!("reed_solomon::compute{i}"))
        .num_threads(opts.num_threads)
        .build()
        .unwrap();

    // Channel used to maintain the order of the frames as they are processed. Jobs are waited
    // for in the order they were submitted
    let (jobs_tx, jobs_rx) = crossbeam::channel::bounded(opts.buffer_size * 2);

    let rs: Arc<dyn ReedSolomon> = match opts.backend {
        RsBackend::Rs2 => Arc::new(
            DefaultReedSolomon::new(opts.interleave)
                .with_detection(opts.detect)
                .with_correction(opts.correct)
                .with_virtual_fill(opts.virtual_fill),
        ),
    };

    // Frame jobs are submitted in a background thread to the compute thread pool. For each job
    // a new "future" channel is created to receive the result of the RS computation. Results are
    // sent to `jobs_tx` in the order they were submitted, and then recieved on `jobs_rx` in that
    // same order, thereby preserving the original frame order.
    std::thread::Builder::new()
        .name("reed_solomon::submit".into())
        .spawn(move || {
            for data in frames {
                let rs = rs.clone();
                let (job_tx, job_rx) = crossbeam::channel::bounded(1);
                pool.spawn(move || {
                    let (integrity, data) = match rs.perform(&data) {
                        Ok(v) => v,
                        Err(err) => panic!("rs failed: {err:?}"),
                    };

                    job_tx
                        .send(RsData { data, integrity })
                        .expect("failed to send frame");
                });

                // NOTE: Every job_rx channel must have a corresponding job_tx channel to which a
                // job was sent, otherwise the receive in the loop below will block forever.
                if jobs_tx.send(job_rx).is_err() {
                    debug!("failed to send job to output channel, exiting");
                    break;
                }
            }
        })
        .expect("expected to be able to create a thread");

    // Wait for job results in submit order, sending resulting frames to the output channel.
    for job in jobs_rx {
        if let Ok(frame) = job.recv() {
            let _ = result_tx.send(frame);
            continue;
        }
        debug!("failed to receive frame from job, exiting");
        break;
    }
}

/// Perform ReedSolomon error correction using [DefaultReedSolomon].
///
/// Works with any `Vec<u8>`.
///
/// RS is the most computationally expensive operation in the decoding process. A pool of
/// background threads is used to perform the algorithm in parallel. Each individual frame of data
/// is a job in the background pool. The number of threads used for the RS computation can be set
/// using [RsOpts::with_num_threads].
///
/// # Arguments
/// - `data` [Iterator] of `Vec<u8>` containing interleved RS message blocks and parity bytes.
/// - `opts` Configuration for the ReedSolomon algorithm. For details see the associated
///   configuration functions on [DefaultReedSolomon].
///
/// # Example
/// ```
/// use ccsds::framing::{reed_solomon, Integrity, RsOpts, RsData};
///
/// let data_in = vec![vec![1u8; 1020]];
/// let data_out: Vec<RsData> = reed_solomon(data_in.into_iter(), RsOpts::new(4)).collect();
///
/// assert_eq!(data_out.len(), 1);
/// assert!(matches!(data_out[0].integrity, Integrity::Ok), "got {:?}", data_out[0].integrity);
/// ```
pub fn reed_solomon<I>(data: I, opts: RsOpts) -> impl Iterator<Item = RsData>
where
    I: Iterator<Item = Vec<u8>> + Send + 'static,
{
    let (output_tx, output_rx) = crossbeam::channel::bounded(opts.buffer_size);

    std::thread::Builder::new()
        .name("reed_solomon::dispatch".into())
        .spawn(move || {
            do_reed_solomon(data, opts, output_tx);
            debug!("reed_solomon::dispatch thread exit");
        })
        .unwrap();

    output_rx.into_iter()
}

pub fn packet_decoder<I>(
    frames: I,
    izone_length: usize,
    ocf: bool,
    fec: bool,
    fhec: bool,
) -> impl Iterator<Item = Packet> + Send + 'static
where
    I: Iterator<Item = Frame> + Send + 'static,
{
    PacketDemux::new(frames).with_defaults(izone_length, fhec, ocf, fec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::{synchronizer::Loc, Block};

    /// AOS header (version 1) with the given vcid and 24-bit counter.
    fn aos(vcid: u8, counter: u32) -> Block {
        let c = counter.to_be_bytes();
        Block {
            last: 0,
            loc: Loc { offset: 1, bit: 0 },
            data: vec![0x40, vcid, c[1], c[2], c[3], 0x00],
        }
    }

    /// TM header (version 0) with the given vcid and virtual channel count.
    fn tm(vcid: u8, mc_count: u8, vc_count: u8) -> Block {
        Block {
            last: 0,
            loc: Loc { offset: 1, bit: 0 },
            data: vec![0x00, vcid << 1, mc_count, vc_count, 0x00, 0x00],
        }
    }

    /// Returns `(missing, duplicate)` for each frame.
    fn run(cadus: Vec<Block>) -> Vec<(u32, bool)> {
        frame_decoder(cadus.into_iter().map(|c| c.data))
            .filter_map(|f| match f.state {
                Some(c) => Some((c.missing, c.duplicate)),
                None => None,
            })
            .collect()
    }

    #[test]
    fn first_frame_has_no_missing_or_duplicate() {
        assert_eq!(run(vec![aos(1, 10)]), vec![(0, false)]);
    }

    #[test]
    fn counts_missing_and_duplicate() {
        let zult = run(vec![aos(1, 10), aos(1, 11), aos(1, 11), aos(1, 14)]);
        assert_eq!(zult, vec![(0, false), (0, false), (0, true), (2, false)]);
    }

    #[test]
    fn aos_counter_wrap() {
        let zult = run(vec![aos(1, 0xff_fffe), aos(1, 0xff_ffff), aos(1, 0)]);
        assert_eq!(zult, vec![(0, false), (0, false), (0, false)]);
    }

    #[test]
    fn vcids_are_tracked_independently() {
        let zult = run(vec![aos(1, 10), aos(2, 50), aos(1, 11), aos(2, 50)]);
        assert_eq!(zult, vec![(0, false), (0, false), (0, false), (0, true)]);
    }

    #[test]
    fn fill_frames_are_never_missing_or_duplicate() {
        let zult = run(vec![aos(63, 5), aos(63, 5), aos(63, 100)]);
        assert_eq!(zult, vec![(0, false); 3]);
    }

    #[test]
    fn tm_uses_virtual_channel_count_that_wraps_at_256() {
        // The master channel count changes on every frame of every VCID and must not matter.
        let zult = run(vec![
            tm(1, 200, 254),
            tm(2, 201, 9),
            tm(1, 202, 255),
            tm(1, 203, 0),
            tm(1, 204, 0),
            tm(1, 205, 3),
        ]);
        assert_eq!(
            zult,
            vec![
                (0, false),
                (0, false),
                (0, false),
                (0, false),
                (0, true),
                (2, false),
            ]
        );
    }
}
