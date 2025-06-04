use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
};

use tracing::{debug, trace};

use crate::framing::{Integrity, Vcid};
use crate::spacepacket::{Packet, PrimaryHeader};

use super::Frame;

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

    fn reset(&mut self) {
        self.cache.clear();
        self.sync = false;
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

pub(crate) struct FramedPacketIter<I>
where
    I: Iterator<Item = Frame>,
{
    frames: I,
    izone_length: usize,
    trailer_length: usize,

    // Cache of partial packet data from frames that has not yet been decoded into
    // packets. There should only be up to about 1 frame worth of data in the cache
    cache: HashMap<Vcid, VcidTracker>,
    // Packets that have already been decoded and are waiting to be provided.
    ready: VecDeque<Packet>,
}

impl<I> FramedPacketIter<I>
where
    I: Iterator<Item = Frame>,
{
    pub fn new(frames: I, izone_length: usize, trailer_length: usize) -> Self {
        FramedPacketIter {
            frames,
            izone_length,
            trailer_length,
            cache: HashMap::default(),
            ready: VecDeque::default(),
        }
    }
}

impl<I> Iterator for FramedPacketIter<I>
where
    I: Iterator<Item = Frame>,
{
    type Item = Packet;

    fn next(&mut self) -> Option<Self::Item> {
        // If there are packets ready to go provide the oldest one
        if let Some(packet) = self.ready.pop_front() {
            return Some(packet);
        }

        // No packet ready, we have to find one
        'next_frame: loop {
            let frame = self.frames.next();
            let Some(frame) = frame else {
                trace!("no more frames");
                break;
            };

            let mpdu = frame.mpdu(self.izone_length, self.trailer_length).unwrap();
            let tracker = self
                .cache
                .entry(frame.header.vcid)
                .or_insert(VcidTracker::new(frame.header.vcid));

            match frame.integrity {
                Some(Integrity::Corrected) => {
                    debug!(vcid = %frame.header.vcid, "corrected frame");
                    tracker.rs_corrected = true;
                }
                Some(Integrity::Uncorrectable | Integrity::NotCorrected) => {
                    debug!(vcid = %frame.header.vcid, tracker = %tracker, "uncorrectable or errored frame, dropping tracker");
                    tracker.reset();
                    continue;
                }
                _ => {}
            }
            // Frame error indicates there are frames missing _before_ this one -- this one is
            // still useable, so clear the existing cache and continue to process this frame.
            if frame.missing > 0 {
                trace!(vcid = frame.header.vcid, tracker=%tracker, missing=frame.missing, "missing frames, dropping tracker");
                tracker.reset();
            }

            if tracker.sync {
                // If we have sync, add the MPDU data to the current tracker
                tracker.cache.extend_from_slice(mpdu.payload());
            } else {
                // No sync, check for the presence of a FPH (first packet header).

                // No way to get sync if we don't have a packet header
                if !mpdu.has_header() {
                    trace!(vcid = %frame.header.vcid, tracker = %tracker, "frames w/o mpdu, dropping");
                    continue;
                }
                // I don't think there should ever be a fill MPDU in a non-fill VCDU, but we check
                // anyways.
                if mpdu.is_fill() {
                    trace!(vcid = %frame.header.vcid, tracker = %tracker, "fill mpdu, dropping");
                    continue;
                }

                if mpdu.header_offset() > mpdu.payload().len() {
                    debug!(
                        "invalid MPDU header offset; value={} buf size={}",
                        mpdu.header_offset(),
                        mpdu.payload().len()
                    );
                    continue;
                }

                // We have valid packet header, so we have sync; init the cache
                tracker.sync = true;
                tracker.cache = mpdu.payload()[mpdu.header_offset()..].to_vec();
            }

            // Handle the case where there are not enough bytes to read a complete header and
            // just collect the next frame. I'm not sure if this should really happen, but we
            // cover the case anyways.
            if tracker.cache.len() < PrimaryHeader::LEN {
                continue 'next_frame;
            }

            // The start of the cache should always contain a packet primary header
            let mut header =
                PrimaryHeader::decode(&tracker.cache).expect("failed to decode primary header");
            if !valid_packet_header(&header) {
                tracker.reset();
                continue;
            }

            // TODO: Add packet validations for length, version, and type

            // Make sure we have enough data to fully construct the packet indicated by the header
            let mut need = header.len_minus1 as usize + 1 + PrimaryHeader::LEN;
            if tracker.cache.len() < need {
                continue 'next_frame;
            }

            // The tracker cache has enough data to construct at least the first packet available
            // in the tracker cache. It's possible the cache also has enough data for additional
            // packets as well, so continue constructing packets from the cache while there is more
            // cache data available. Created packets are pushed onto the ready queue.
            loop {
                // data is for the current packet, tail is what's left of the cache
                let (data, tail) = tracker.cache.split_at(need);
                let packet = Packet {
                    header: PrimaryHeader::decode(data).expect("failed to decode primary header"),
                    data: data.to_vec(),
                    offset: 0,
                };
                tracker.cache = tail.to_vec();
                self.ready.push_back(packet);

                if tracker.cache.len() < PrimaryHeader::LEN {
                    break;
                }
                header =
                    PrimaryHeader::decode(&tracker.cache).expect("failed to decode primary header");
                if !valid_packet_header(&header) {
                    tracker.reset();
                    break;
                }
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

fn valid_packet_header(header: &PrimaryHeader) -> bool {
    if header.version != 0 || header.type_flag != 0 {
        debug!("bad packet version or type, dropping {header:?}");
        return false;
    }
    true
}
