use std::sync::Arc;

use crossbeam::channel::Sender;
use tracing::debug;

use crate::framing::{DefaultReedSolomon, Frame, Integrity, ReedSolomon};

/// Configuration options for the ReedSolomon supported by [super::Pipeline].
#[derive(Debug, Clone, Copy)]
pub struct RsOpts {
    interleave: u8,
    virtual_fill: usize,
    num_threads: usize,
    buffer_size: usize,
    detect: bool,
    correct: bool,
}

impl RsOpts {
    pub fn new(interleave: u8) -> Self {
        RsOpts {
            interleave,
            virtual_fill: 0,
            num_threads: 0,
            detect: true,
            correct: true,
            buffer_size: 1024,
        }
    }

    /// See [DefaultReedSolomon::with_virtual_fill]
    pub fn with_virtual_fill(mut self, virtual_fill: usize) -> Self {
        self.virtual_fill = virtual_fill;
        self
    }

    /// Size of the thread pool used to perform the RS compuataion. By default the value will be
    /// chosen automatically.
    pub fn with_num_threads(mut self, num_threads: usize) -> Self {
        self.num_threads = num_threads;
        self
    }

    /// See [DefaultReedSolomon::with_correction]
    pub fn with_correction(mut self, enabled: bool) -> Self {
        self.correct = enabled;
        self
    }

    /// See [DefaultReedSolomon::with_detection]
    pub fn with_detection(mut self, enabled: bool) -> Self {
        self.detect = enabled;
        self
    }

    /// Set the allowable number of in-flight frames waiting to enter the thread pool.
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }
}

fn do_reed_solomon<I>(frames: I, opts: RsOpts, tx: Sender<Frame>)
where
    I: Iterator<Item = Frame> + Send + 'static,
{
    let pool = rayon::ThreadPoolBuilder::new()
        .thread_name(|i| format!("reed_solomon::compute{i}"))
        .num_threads(opts.num_threads)
        .build()
        .unwrap();
    let rs = Arc::new(
        DefaultReedSolomon::new(opts.interleave)
            .with_detection(opts.detect)
            .with_correction(opts.correct)
            .with_virtual_fill(opts.virtual_fill),
    );

    for mut frame in frames {
        let tx = tx.clone();
        let rs = rs.clone();
        pool.spawn_fifo(move || {
            let (integrity, data) = match rs.perform(&frame.header, &frame.data) {
                Ok(v) => v,
                Err(err) => panic!("rs failed: {err:?}"),
            };
            frame.integrity = Some(integrity);

            // data does not include the check symbols
            match frame.integrity {
                Some(Integrity::Ok | Integrity::Corrected) => frame.data = data,
                _ => (),
            }

            if let Err(err) = tx.send(frame) {
                panic!("failed to send: {err:?}");
            }
        })
    }
}

/// Perform ReedSolomon error correction using [DefaultReedSolomon].
///
/// RS is the most computationally expensive operation in the decoding process. A pool of
/// background threads is used to perform the algorithm in parallel. Each individual frame of data
/// is a job in the background pool. The number of threads in the pool can be set using the
/// [RsOpts::with_num_threads].
///
/// # Arguments
/// * `frames` [Iterator] of frames as returned by [framing_decoder](crate::framing).
/// * `opts` Configuration for the ReedSolomon algorithm. For details see the associated
/// configuration functions on [DefaultReedSolomon].
///
/// # Example
/// ```
/// use ccsds::framing::{Frame, reed_solomon, Integrity, RsOpts};
///
/// let frames_in = vec![Frame::decode(vec![1u8; 1020]).unwrap()];
/// let frames_out: Vec<Frame> = reed_solomon(frames_in.into_iter(), RsOpts::new(4)).collect();
///
/// assert_eq!(frames_out.len(), 1);
/// assert!(matches!(frames_out[0].integrity, Some(Integrity::Ok)), "got {:?}",
/// frames_out[0].integrity);
/// ```
pub fn reed_solomon<I>(frames: I, opts: RsOpts) -> impl Iterator<Item = Frame>
where
    I: Iterator<Item = Frame> + Send + 'static,
{
    let (output_tx, output_rx) = crossbeam::channel::bounded(opts.buffer_size);

    std::thread::Builder::new()
        .name("reed_solomon::dispatch".into())
        .spawn(move || {
            do_reed_solomon(frames, opts, output_tx);
            debug!("reed_solomon::dispatch thread exit");
        })
        .unwrap();

    output_rx.into_iter()
}
