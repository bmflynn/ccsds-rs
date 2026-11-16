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
                let mut duplicate = false;
                if header.vcid != VCDUHeader::FILL {
                    if let Some(last) = self.vcid_counters.get(&header.vcid) {
                        duplicate = header.counter == *last;
                        missing = missing_frames(header.counter, *last, header.counter_max());
                    }
                }
                self.vcid_counters.insert(header.vcid, header.counter);
                Some(Frame {
                    header,
                    missing,
                    duplicate,
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
    CaduDecoderIter {
        vcid_counters: HashMap::default(),
        cadus,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::synchronizer::Loc;

    /// AOS header (version 1) with the given vcid and 24-bit counter.
    fn aos(vcid: u8, counter: u32) -> Cadu {
        let c = counter.to_be_bytes();
        Cadu {
            last: 0,
            loc: Loc { offset: 1, bit: 0 },
            data: vec![0x40, vcid, c[1], c[2], c[3], 0x00],
        }
    }

    /// TM header (version 0) with the given vcid and virtual channel count.
    fn tm(vcid: u8, mc_count: u8, vc_count: u8) -> Cadu {
        Cadu {
            last: 0,
            loc: Loc { offset: 1, bit: 0 },
            data: vec![0x00, vcid << 1, mc_count, vc_count, 0x00, 0x00],
        }
    }

    /// Returns `(missing, duplicate)` for each frame.
    fn run(cadus: Vec<Cadu>) -> Vec<(u32, bool)> {
        frame_decoder(cadus.into_iter())
            .map(|f| (f.missing, f.duplicate))
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
