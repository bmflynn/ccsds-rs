use std::sync::Arc;

use tracing::debug;

use crate::framing::{DefaultReedSolomon, Frame, Integrity, ReedSolomon};

struct ReedSolomonIter {
    frames: crossbeam::channel::Receiver<Frame>,
}

impl ReedSolomonIter {
    pub fn new<I>(frames: I, interleave: u8, virtual_fill: usize) -> Self
    where
        I: Iterator<Item = Frame> + Send + 'static,
    {
        let (output_tx, output_rx) = crossbeam::channel::bounded(1024);

        std::thread::spawn(move || {
            let pool = rayon::ThreadPoolBuilder::new().build().unwrap();
            let rs = Arc::new(DefaultReedSolomon::new(interleave).with_virtual_fill(virtual_fill));

            for mut frame in frames {
                debug!("rs: {:?}", frame.header);
                let output_tx = output_tx.clone();
                let rs = rs.clone();
                pool.spawn_fifo(move || {
                    let Ok((integrity, data)) = rs.perform(&frame.header, &frame.data) else {
                        return;
                    };
                    frame.integrity = Some(integrity.into());

                    // data does not include the check symbols
                    match frame.integrity {
                        Some(Integrity::Ok | Integrity::Corrected) => frame.data = data,
                        _ => (),
                    }

                    let _ = output_tx.send(frame);
                })
            }
        });

        Self { frames: output_rx }
    }
}

impl Iterator for ReedSolomonIter {
    type Item = Frame;

    fn next(&mut self) -> Option<Self::Item> {
        match self.frames.recv() {
            Ok(frame) => Some(frame),
            Err(_) => None,
        }
    }
}

pub fn reed_solomon<I>(
    frames: I,
    interleave: u8,
    virtual_fill: usize,
) -> impl Iterator<Item = Frame>
where
    I: Iterator<Item = Frame> + Send + 'static,
{
    ReedSolomonIter::new(frames, interleave, virtual_fill)
}
