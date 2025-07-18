use std::collections::HashMap;

use crate::framing::{missing_frames, Cadu, Frame, VCDUHeader};

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
                let mut missing = 0;
                if !header.vcid == VCDUHeader::FILL && self.vcid_counters.contains_key(&header.vcid) {
                    let last = self.vcid_counters.get(&header.vcid).unwrap();
                    missing = missing_frames(header.counter, *last);
                } 
                self.vcid_counters.insert(header.vcid, header.counter);
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
