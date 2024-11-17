use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
};

use tracing::{debug, trace};

use crate::framing::{integrity::Integrity, DecodedFrame, Scid, Vcid};
use crate::spacepacket::{Packet, PrimaryHeader};

struct VcidTracker {
    vcid: Vcid,
    /// Caches partial packets for this vcid
    cache: Vec<u8>,
    // True when any frame used to fill the cache was rs corrected
    rs_corrected: bool,
    // True when a FHP has been found and data should be added to cache. False
    // where there is a missing data due to RS failure or missing frames.
    sync: bool,
}

impl VcidTracker {
    fn new(vcid: Vcid) -> Self {
        VcidTracker {
            vcid,
            sync: false,
            cache: vec![],
            rs_corrected: false,
        }
    }

    fn clear(&mut self) {
        self.cache.clear();
        self.rs_corrected = false;
    }
}

impl Display for VcidTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "VcidTracker{{vcid={}, sync={}, cache_len={}, rs_corrected:{}}}",
            self.vcid,
            self.sync,
            self.cache.len(),
            self.rs_corrected
        )
    }
}

/// A [Packet] with additional framing metadata.
#[derive(Debug, Clone)]
pub struct DecodedPacket {
    pub scid: Scid,
    pub vcid: Vcid,
    pub packet: Packet,
}

struct FramedPacketIter<I>
where
    I: Iterator<Item = DecodedFrame> + Send,
{
    frames: I,
    izone_length: usize,
    trailer_length: usize,

    // Cache of partial packet data from frames that has not yet been decoded into
    // packets. There should only be up to about 1 frame worth of data in the cache
    cache: HashMap<Vcid, VcidTracker>,
    // Packets that have already been decoded and are waiting to be provided.
    ready: VecDeque<DecodedPacket>,
}

impl<I> Iterator for FramedPacketIter<I>
where
    I: Iterator<Item = DecodedFrame> + Send,
{
    type Item = DecodedPacket;

    fn next(&mut self) -> Option<Self::Item> {
        // If there are ready packets provide the oldest one
        if let Some(packet) = self.ready.pop_front() {
            return Some(packet);
        }

        // No packet ready, we have to find one
        'next_frame: loop {
            let frame = self.frames.next();
            if frame.is_none() {
                trace!("no more frames");
                break;
            }

            let DecodedFrame {
                frame,
                missing,
                integrity,
            } = frame.unwrap();
            let mpdu = frame.mpdu(self.izone_length, self.trailer_length).unwrap();
            let tracker = self
                .cache
                .entry(frame.header.vcid)
                .or_insert(VcidTracker::new(frame.header.vcid));

            match integrity {
                Some(Integrity::Corrected) => {
                    debug!(vcid = %frame.header.vcid, "corrected frame");
                    tracker.rs_corrected = true;
                }
                Some(Integrity::Uncorrectable) | Some(Integrity::HasErrors) => {
                    debug!(vcid = %frame.header.vcid, tracker = %tracker, "uncorrectable or errored frame, dropping tracker");
                    tracker.clear();
                    tracker.sync = false;
                    continue;
                }
                _ => {}
            }
            // For counter errors, we can still utilize the current frame (no continue)
            if missing > 0 {
                trace!(vcid = frame.header.vcid, tracker=%tracker, missing=missing, "missing frames, dropping tracker");
                tracker.clear();
                tracker.sync = false;
            }

            if tracker.sync {
                // If we have sync all mpdu bytes are for this tracker/vcid
                tracker.cache.extend_from_slice(mpdu.payload());
            } else {
                // No way to get sync if we don't have a header
                if !mpdu.has_header() {
                    trace!(vcid = %frame.header.vcid, tracker = %tracker, "frames w/o mpdu, dropping");
                    continue;
                }

                if mpdu.header_offset() > mpdu.payload().len() {
                    panic!("MPDU header offset too large; likely due to an incorrect frame length; offset={} buf size={}",
                        mpdu.header_offset(),  mpdu.payload().len()
                    );
                }
                tracker.cache = mpdu.payload()[mpdu.header_offset()..].to_vec();
                tracker.sync = true;
            }

            if tracker.cache.len() < PrimaryHeader::LEN {
                continue 'next_frame; // need more frame data for this vcid
            }
            let mut header =
                PrimaryHeader::decode(&tracker.cache).expect("failed to decode primary header");
            let mut need = header.len_minus1 as usize + 1 + PrimaryHeader::LEN;
            if tracker.cache.len() < need {
                continue; // need more frame data for this vcid
            }

            loop {
                // Grab data we need and update the cache
                let (data, tail) = tracker.cache.split_at(need);
                let packet = DecodedPacket {
                    scid: frame.header.scid,
                    vcid: frame.header.vcid,
                    packet: Packet {
                        header: PrimaryHeader::decode(data)
                            .expect("failed to decode primary header"),
                        data: data.to_vec(),
                        offset: 0,
                    },
                };
                tracker.cache = tail.to_vec();
                self.ready.push_back(packet);

                if tracker.cache.len() < PrimaryHeader::LEN {
                    break;
                }
                header =
                    PrimaryHeader::decode(&tracker.cache).expect("failed to decode primary header");
                need = header.len_minus1 as usize + 1 + PrimaryHeader::LEN;
                if tracker.cache.len() < need {
                    break;
                }
            }

            return self.ready.pop_front();
        }

        // Attempted to read a frame, but the iterator is done.  Make sure to
        // provide a ready frame if there are any.
        self.ready.pop_front()
    }
}

/// Decodes the provided frames into a packets contained within the frames' MPDUs.
///
/// While not strictly enforced, frames should all be from the same spacecraft, i.e., have
/// the same spacecraft id.
///
/// There are several cases when frame data cannot be fully recovered and is dropped,
/// i.e., not used to construct packets:
///
/// 1. Missing frames
/// 2. Frames with state ``rs2::RSState::Uncorrectable``
/// 3. Fill Frames
/// 4. Frames before the first header is available in an MPDU
pub fn decode_framed_packets<I>(
    frames: I,
    izone_length: usize,
    trailer_length: usize,
) -> impl Iterator<Item = DecodedPacket> + Send
where
    I: Iterator<Item = DecodedFrame> + Send,
{
    FramedPacketIter {
        frames: frames.filter(|dc| !dc.frame.is_fill()),
        izone_length,
        trailer_length,
        cache: HashMap::new(),
        ready: VecDeque::new(),
    }
}
