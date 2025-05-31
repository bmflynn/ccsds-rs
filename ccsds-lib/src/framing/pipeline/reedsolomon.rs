use std::sync::Arc;

use crossbeam::channel::Sender;

use crate::framing::{DefaultReedSolomon, Frame, Integrity, ReedSolomon};

#[derive(Debug, Clone, Copy)]
pub struct RsOpts {
    interleave: u8,
    virtual_fill: usize,
    num_threads: usize,
}

impl RsOpts {
    pub fn new(interleave: u8) -> Self {
        RsOpts {
            interleave,
            virtual_fill: 0,
            num_threads: 0,
        }
    }

    pub fn virtual_fill(mut self, virtual_fill: usize) -> Self {
        self.virtual_fill = virtual_fill;
        self
    }

    pub fn num_threads(mut self, num_threads: usize) -> Self {
        self.num_threads = num_threads;
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
    let rs =
        Arc::new(DefaultReedSolomon::new(opts.interleave).with_virtual_fill(opts.virtual_fill));

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

pub fn reed_solomon<I>(frames: I, opts: RsOpts) -> impl Iterator<Item = Frame>
where
    I: Iterator<Item = Frame> + Send + 'static,
{
    let (output_tx, output_rx) = crossbeam::channel::bounded(100);

    std::thread::Builder::new()
        .name("reed_solomon::dispatch".into())
        .spawn(move || do_reed_solomon(frames, opts, output_tx))
        .unwrap();

    output_rx.into_iter()
}
