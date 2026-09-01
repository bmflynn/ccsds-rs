use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
};

use tracing::{debug, trace};

use crate::framing::{Integrity, Vcid, FEC_LEN, MPDU, OCF_LEN};
use crate::spacepacket::{Packet, PrimaryHeader};

use super::Frame;

/// Per-channel framing configuration
#[derive(Clone, Debug, Default)]
struct ChannelConfig {
    izone_len: usize,
    // Frame header error correction
    fhec: bool,
    // Frame error correction
    fec: bool,
    // Operational control field
    ocf: bool,
}

/// Demux packets from frames.
///
/// Packets are decoded in the order in which they are received, per VCID.
///
/// Packet data may be dropped/lost in the following cases:
///
/// * Not enough data left to construct an entire frame.
/// * Not enough data left to construct an entire packet.
/// * Not enough data within the frame to construct a packet primary header.
/// * Frame received with that contains errors ([Integrity::Uncorrectable](crate::framing),
///   [Integrity::NotCorrected](crate::framing))
/// * Invalid MPDU first header pointer value
/// * Discontinuity in the frame counter from the current frame to the previous frame of the same
///   VCID.
///
/// # Example
/// ```
/// use ccsds::framing::{Frame, PacketDemux};
/// use ccsds::spacepacket::Packet;
///
/// let frames = vec![Frame::decode(vec![0u8; 1020]).unwrap()];
/// let packets: Vec<Packet> = PacketDemux::new(frames.into_iter()).with_defaults(0, false, false, false).collect();
/// ```
#[derive(Clone, Debug)]
pub struct PacketDemux<I>
where
    I: Iterator<Item = Frame> + Send,
{
    frames: I,

    // Cache of partial packet data from frames that has not yet been decoded into
    // packets. There should only be up to about 1 frame worth of data in the cache
    cache: HashMap<Vcid, VcidTracker>,
    // Packets that have already been decoded and are waiting to be provided.
    ready: VecDeque<Packet>,
    default_config: Option<ChannelConfig>,
    channel_config: HashMap<Vcid, ChannelConfig>,
}

impl<I> PacketDemux<I>
where
    I: Iterator<Item = Frame> + Send,
{
    /// Create a new [PacketDemux].
    ///
    /// # Arguments:
    /// * `frames` - Frames to demux packets from
    pub fn new(frames: I) -> Self {
        Self {
            frames,
            cache: HashMap::default(),
            ready: VecDeque::default(),
            channel_config: HashMap::default(),
            default_config: None,
        }
    }

    /// Set the default framing configuration if no per-channel configuration is avaialble.
    ///
    /// # Arguments
    /// * `izone_len` - Insert zone length in bytes.
    /// * `fhec` - Frame header contains frame header error control bytes
    /// * `ocf` - Frame trailer contains operational control field bytes
    /// * `fec` - Frame trailer contains frame error control bytes
    pub fn with_defaults(mut self, izone_len: usize, fhec: bool, ocf: bool, fec: bool) -> Self {
        self.default_config = Some(ChannelConfig {
            izone_len,
            fhec,
            ocf,
            fec,
        });
        self
    }

    /// Add an chanel specific framing configuration.
    pub fn with_channel_config(
        mut self,
        vcid: Vcid,
        izone_len: usize,
        fhec: bool,
        ocf: bool,
        fec: bool,
    ) -> PacketDemux<I> {
        self.channel_config.insert(
            vcid,
            ChannelConfig {
                izone_len,
                fhec,
                ocf,
                fec,
            },
        );
        self
    }

    /// Get a frames MPDU based on the channel config
    fn mpdu(&self, frame: &Frame) -> Option<MPDU> {
        let cfg = self
            .channel_config
            .get(&frame.header.vcid)
            .or(self.default_config.as_ref())?;
        let trailer_len = match (cfg.ocf, cfg.fec) {
            (true, true) => OCF_LEN + FEC_LEN,
            (true, false) => OCF_LEN,
            (false, true) => FEC_LEN,
            (false, false) => 0,
        };
        frame.mpdu(cfg.izone_len, trailer_len, cfg.fhec)
    }
}

impl<I> Iterator for PacketDemux<I>
where
    I: Iterator<Item = Frame> + Send,
{
    type Item = Packet;

    fn next(&mut self) -> Option<Self::Item> {
        // If there are packets ready to go, provide the oldest one
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

            let mpdu = match self.mpdu(&frame) {
                Some(m) => m,
                None => {
                    debug!("demux failed to extract mpdu from frame");
                    return None;
                }
            };
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

#[derive(Clone, Debug)]
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

fn valid_packet_header(header: &PrimaryHeader) -> bool {
    if header.version != 0 || header.type_flag != 0 {
        debug!("bad packet version or type, dropping {header:?}");
        return false;
    }
    true
}
