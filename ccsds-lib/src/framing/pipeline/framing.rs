use std::collections::HashMap;

use crate::framing::{missing_frames, Frame, VCDUHeader};

use super::Cadu;

struct CaduDecoderIter<I>
where
    I: Iterator<Item = Cadu> + Send + 'static,
{
    vcid_counters: HashMap<u16, u32>,
    cadus: I,
}

impl<I> Iterator for CaduDecoderIter<I>
where
    I: Iterator<Item = Cadu> + Send + 'static,
{
    type Item = Frame;

    fn next(&mut self) -> Option<Self::Item> {
        let cadu = self.cadus.next()?;
        match VCDUHeader::decode(&cadu.data) {
            Some(header) => {
                let last = self
                    .vcid_counters
                    .entry(header.vcid)
                    .or_insert(header.counter);
                let missing = missing_frames(header.counter, *last);
                Some(Frame {
                    header,
                    missing,
                    integrity: None,
                    data: cadu.data,
                })
            }
            None => None,
        }
    }
}

/// Decode [Cadu]s into [Frame]s.
pub fn frame_decoder<I>(cadus: I) -> impl Iterator<Item = Frame> + Send + 'static
where
    I: Iterator<Item = Cadu> + Send + 'static,
{
    let iter = CaduDecoderIter {
        vcid_counters: HashMap::default(),
        cadus,
    };

    iter
}
