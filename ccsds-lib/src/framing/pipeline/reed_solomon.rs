use std::sync::Arc;

#[cfg(feature = "python")]
use pyo3::{prelude::*, pyclass};

use crossbeam::channel::Sender;
use tracing::debug;

use crate::framing::{DefaultReedSolomon, Frame, Integrity, ReedSolomon};

/// Configuration options for the ReedSolomon supported by [super::Pipeline].
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "python", pyclass(get_all, set_all))]
pub struct RsOpts {
    pub interleave: u8,
    pub virtual_fill: usize,
    pub num_threads: usize,
    pub buffer_size: usize,
    pub detect: bool,
    pub correct: bool,
}

impl RsOpts {
    pub fn new(interleave: u8) -> Self {
        RsOpts {
            interleave,
            virtual_fill: 0,
            num_threads: 0,
            detect: true,
            correct: true,
            buffer_size: 50,
        }
    }
}

#[cfg_attr(feature = "python", pymethods)]
impl RsOpts {
    #[cfg(feature = "python")]
    #[new]
    fn py_new(interleave: u8) -> Self {
        Self::new(interleave)
    }

    /// See [DefaultReedSolomon::with_virtual_fill]
    pub fn with_virtual_fill(&self, virtual_fill: usize) -> Self {
        let mut slf = self.clone();
        slf.virtual_fill = virtual_fill;
        slf
    }

    /// Size of the thread pool used to perform the RS compuataion. By default the value will be
    /// chosen automatically.
    pub fn with_num_threads(&self, num_threads: usize) -> Self {
        let mut slf = self.clone();
        slf.num_threads = num_threads;
        slf
    }

    /// See [DefaultReedSolomon::with_correction]
    pub fn with_correction(&self, enabled: bool) -> Self {
        let mut slf = self.clone();
        slf.correct = enabled;
        slf
    }

    /// See [DefaultReedSolomon::with_detection]
    pub fn with_detection(&self, enabled: bool) -> Self {
        let mut slf = self.clone();
        slf.detect = enabled;
        slf
    }

    /// Set the allowable number of in-flight frames waiting to enter the thread pool.
    pub fn with_buffer_size(&self, size: usize) -> Self {
        let mut slf = self.clone();
        slf.buffer_size = size;
        slf
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

            let _ = tx.send(frame);
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
pub fn reed_solomon<I>(frames: I, opts: RsOpts) -> impl Iterator<Item = Frame> + Send + 'static
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
