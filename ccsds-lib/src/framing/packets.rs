use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
};

use tracing::{debug, trace};

use crate::framing::{Integrity, Vcid};
use crate::spacepacket::{Packet, PrimaryHeader};

use super::Frame;

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct FramedPacketIter<I> 
where
    I: Iterator<Item = Frame>,
{
    frames: I,
    extractor: PacketExtractor,
    ready: VecDeque<Packet>,
}

impl<I> FramedPacketIter<I>
where
    I: Iterator<Item = Frame>,
{
    pub fn new(frames: I, izone_length: usize, trailer_length: usize) -> Self {
        Self {
            frames,
            extractor: PacketExtractor::new(izone_length, trailer_length), 
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

        loop {
            let frame = self.frames.next();
            let Some(frame) = frame else {
                break;
            };

            match self.extractor.handle(&frame) {
                ExtractResult::Drop(reason) => {
                    debug!(vcid=&frame.header.vcid, "frame dropped: {reason}");
                    continue;
                }
                ExtractResult::Packets(packets) => {
                    self.ready.extend(packets);
                    if let Some(packet) = self.ready.pop_front() {
                        return Some(packet);
                    }
                }
                ExtractResult::None => {
                    // No packets ready, continue to the next frame
                    continue;
                }
            }
        }

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

#[derive(Debug, Clone)]
/// Result of processing a frame in the packet extraction pipeline.
pub enum ExtractResult {
    /// A frame was dropped due to an error or data discontinuity.
    Drop(String),
    /// One or more packets were extracted from the frame cache
    Packets(Vec<Packet>),
    /// No packets were extracted, but the frame was processed successfully.
    None,
}

/// Extracts packets from frames.
/// 
/// A cache is maintained of partial packets data that have not yet been decoded into
/// into valid [Packet]s. As frames are processed, the cache is updated with new data
/// and packets are extracted from the cache when enough data is available to construct
/// a complete packet. The cache is keyed by VCID, so packets from different VCIDs are
/// not mixed together. The cache is cleared when a frame is received that has a
/// discontinuity in the frame counter, or when a frame is received that has an
/// integrity error (e.g., [Integrity::Uncorrectable](crate::framing) or
/// [Integrity::NotCorrected](crate::framing)).
#[derive(Debug, Clone)]
pub struct PacketExtractor {
    izone_length: usize,
    trailer_length: usize,

    // Cache of partial packet data from frames that has not yet been decoded into
    // packets. There should only be up to about 1 frame worth of data in the cache
    cache: HashMap<Vcid, VcidTracker>,
}

impl PacketExtractor {
    pub fn new(izone_length: usize, trailer_length: usize) -> Self {
        PacketExtractor {
            izone_length,
            trailer_length,
            cache: HashMap::new(),
        }
    }

    /// Add a frame's data to the internal cache and return all packets that can be constructed
    /// from the current cache state.
    /// 
    /// # Arguments
    /// * `frame`: The frame to process.
    /// 
    /// # Returns
    /// A [ExtractResult] indicating whether packets were extracted, the frame was dropped, or
    /// no packets were extracted but the frame was processed successfully.
    pub fn handle(&mut self, frame: &Frame) -> ExtractResult {

        let mpdu = frame.mpdu(self.izone_length, self.trailer_length).unwrap();
        let tracker = self
            .cache
            .entry(frame.header.vcid)
            .or_insert(VcidTracker::new(frame.header.vcid));

        match frame.integrity {
            Some(Integrity::Corrected) => {
                tracker.rs_corrected = true;
            }
            Some(Integrity::Uncorrectable | Integrity::NotCorrected) => {
                tracker.reset();
                return ExtractResult::Drop("Uncorrectable frame".into());
            }
            _ => {}
        }

        // Frame error indicates there are frames missing _before_ this one -- this one is
        // still useable, so clear the existing cache and continue to process this frame.
        if frame.missing > 0 {
            tracker.reset();
        }

        if tracker.sync {
            // If we have sync, add the MPDU data to the current tracker
            tracker.cache.extend_from_slice(mpdu.payload());
        } else {
            // No sync, check for the presence of a FPH (first packet header).

            // No way to get sync if we don't have a packet header
            if !mpdu.has_header() {
                return ExtractResult::Drop("MDPU without packet header".into()); 
            }
            // I don't think there should ever be a fill MPDU in a non-fill VCDU, but we check
            // anyways.
            if mpdu.is_fill() {
                trace!(vcid = %frame.header.vcid, tracker = %tracker, "fill mpdu, dropping");
                return ExtractResult::Drop("Fill MPDU in non-fill VCDU".into());
            }

            if mpdu.header_offset() > mpdu.payload().len() {
                debug!(
                    "invalid MPDU header offset; value={} buf size={}",
                    mpdu.header_offset(),
                    mpdu.payload().len()
                );
                return ExtractResult::Drop("Invalid MPDU header offset".into());
            }

            // We have valid packet header, so we have sync; init the cache
            tracker.sync = true;
            tracker.cache = mpdu.payload()[mpdu.header_offset()..].to_vec();
        }

        // Handle the case where there are not enough bytes to read a complete header and
        // just collect the next frame. I'm not sure if this should really happen, but we
        // cover the case anyways.
        if tracker.cache.len() < PrimaryHeader::LEN {
            return ExtractResult::None;
        }

        // The start of the cache should always contain a packet primary header
        let mut header =
            PrimaryHeader::decode(&tracker.cache).expect("failed to decode primary header");
        if !valid_packet_header(&header) {
            tracker.reset();
            return ExtractResult::Drop("Invalid packet header".into());
        }

        // TODO: Add packet validations for length, version, and type

        // Make sure we have enough data to fully construct the packet indicated by the header
        let mut need = header.len_minus1 as usize + 1 + PrimaryHeader::LEN;
        if tracker.cache.len() < need {
            return ExtractResult::None;
        }

        let mut ready = Vec::with_capacity(5);
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
            ready.push(packet);

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

        return ExtractResult::Packets(ready);
    }
}