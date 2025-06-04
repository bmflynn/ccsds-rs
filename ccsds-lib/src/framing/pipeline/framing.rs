use std::collections::HashMap;

use crate::framing::{missing_frames, Cadu, Frame, VCDUHeader};

struct CaduDecoderIter<I>
where
    I: Iterator<Item = Cadu>,
{
    vcid_counters: HashMap<u16, u32>,
    cadus: I,
}

impl<I> Iterator for CaduDecoderIter<I>
where
    I: Iterator<Item = Cadu>,
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

/// Decode input [Cadu] data into [Frame] data.
///
/// There is not much real work here other than keeping track of frame sequence couters to
/// facilitate [Frame::missing] count.
pub fn frame_decoder<I>(cadus: I) -> impl Iterator<Item = Frame>
where
    I: Iterator<Item = Cadu>,
{
    let iter = CaduDecoderIter {
        vcid_counters: HashMap::default(),
        cadus,
    };

    iter
}
