mod merge;

use std::collections::HashMap;
use std::collections::VecDeque;
use std::fmt::Display;
use std::io::{Read, Result as IOResult};

use crate::{timecode, Error};
use crate::{DecodedFrame, SCID, VCID};
use hifitime::Epoch;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

pub use merge::merge_by_timecode;

pub type Apid = u16;

/// Decodes a UTC time in microsecods from a packet.
pub trait TimeDecoder {
    fn decode_time(&self, pkt: &Packet) -> Result<Epoch, Error>;
}

/// ``TimeDocoder`` for the CCSDS Day Segmented timecode with no P-field and 2 bytes
/// of submilliseconds. (See [`Time Code Formats`])
///
/// [`Time Code Formats`]: https://public.ccsds.org/Pubs/301x0b4e1.pdf
pub struct CdsTimeDecoder {
    format: timecode::Format,
    offset: usize,
}

impl Default for CdsTimeDecoder {
    fn default() -> Self {
        Self {
            format: timecode::Format::Cds {
                num_day: 2,
                num_submillis: 2,
            },
            offset: 0,
        }
    }
}

impl TimeDecoder for CdsTimeDecoder {
    fn decode_time(&self, pkt: &Packet) -> Result<Epoch, Error> {
        Ok(
            timecode::decode(&self.format, &pkt.data[PrimaryHeader::LEN + self.offset..])?
                .epoch()?,
        )
    }
}

/// Packet represents a single CCSDS space packet and its associated data.
///
/// This packet contains the primary header data as well as the user data,
/// which may or may not container a secondary header. See the header's
/// `has_secondary_header` flag.
///
/// # Example
/// Create a packet from the minimum number of bytes.
/// ```
/// use ccsds::{Packet, PrimaryHeader};
///
/// let dat: &[u8] = &[
///     // primary header bytes
///     0xd, 0x59, 0xd2, 0xab, 0x0, 07,
///     // Cds timecode bytes in secondary header (not decoded here)
///     0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
///     // minimum 1 byte of user data
///     0xff
/// ];
/// let mut r = std::io::BufReader::new(dat);
/// let packet = Packet::read(&mut r).unwrap();
/// ```
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Packet {
    /// All packets have a primary header
    pub header: PrimaryHeader,
    /// All packet bytes, including header and user data
    pub data: Vec<u8>,

    offset: usize,
}

impl Display for Packet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Packet{{header: {:?}, data:[len={}]}}",
            self.header,
            self.data.len()
        )?;
        Ok(())
    }
}

impl Packet {
    #[must_use]
    pub fn is_first(&self) -> bool {
        self.header.sequence_flags == SEQ_FIRST
    }

    #[must_use]
    pub fn is_last(&self) -> bool {
        self.header.sequence_flags == SEQ_LAST
    }

    #[must_use]
    pub fn is_cont(&self) -> bool {
        self.header.sequence_flags == SEQ_CONTINUATION
    }

    #[must_use]
    pub fn is_standalone(&self) -> bool {
        self.header.sequence_flags == SEQ_UNSEGMENTED
    }

    /// Decode from bytes. Returns `None` if there are not enough bytes to construct the
    /// header or if there are not enough bytes to construct the [Packet] of the length
    /// indicated by the header.
    #[must_use]
    pub fn decode(dat: &[u8]) -> Option<Packet> {
        match PrimaryHeader::decode(dat) {
            Some(header) => {
                if dat.len() < header.len_minus1 as usize + 1 {
                    None
                } else {
                    Some(Packet {
                        header,
                        data: dat.to_vec(),
                        offset: 0,
                    })
                }
            }
            None => None,
        }
    }

    /// Read a single [Packet].
    ///
    /// # Errors
    /// Any ``std::io::Error`` reading
    #[allow(clippy::missing_panics_doc)]
    pub fn read<R>(mut r: R) -> IOResult<Packet>
    where
        R: Read + Send,
    {
        let mut buf = vec![0u8; 65536];
        r.read_exact(&mut buf[..PrimaryHeader::LEN])?;
        // we know there are enough bytes because we just read them
        let ph = PrimaryHeader::decode(&buf[..PrimaryHeader::LEN]).unwrap();
        let data_len = ph.len_minus1 as usize + 1;
        let total_len = PrimaryHeader::LEN + data_len;
        r.read_exact(&mut buf[PrimaryHeader::LEN..total_len])?;

        Ok(Packet {
            header: ph,
            data: buf[..total_len].to_vec(),
            offset: 0,
        })
    }
}

/// Packet is the first packet in a packet group
pub const SEQ_FIRST: u8 = 1;
/// Packet is a part of a packet group, but not first and not last
pub const SEQ_CONTINUATION: u8 = 0;
/// Packet is the last packet in a packet group
pub const SEQ_LAST: u8 = 2;
/// Packet is not part of a packet group, i.e., standalone.
pub const SEQ_UNSEGMENTED: u8 = 3;

/// CCSDS Primary Header
///
/// The primary header format is common to all CCSDS space packets.
///
#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct PrimaryHeader {
    pub version: u8,
    pub type_flag: u8,
    pub has_secondary_header: bool,
    pub apid: Apid,
    /// Defines a packets grouping. See the `SEQ_*` values.
    pub sequence_flags: u8,
    pub sequence_id: u16,
    pub len_minus1: u16,
}

impl PrimaryHeader {
    /// Size of a ``PrimaryHeader``
    pub const LEN: usize = 6;
    pub const SEQ_MAX: u16 = 16383;

    /// Read header from `r`.
    ///
    /// # Errors
    /// Any ``std::io::Error`` reading
    #[allow(clippy::missing_panics_doc)]
    pub fn read<R>(mut r: R) -> IOResult<PrimaryHeader>
    where
        R: Read + Send,
    {
        let mut buf = [0u8; Self::LEN];
        r.read_exact(&mut buf)?;

        // Can't panic because of read_exact
        Ok(Self::decode(&buf).unwrap())
    }

    /// Decode from bytes. Returns `None` if there are not enough bytes to construct the
    /// header.
    #[must_use]
    pub fn decode(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::LEN {
            return None;
        }
        let d1 = u16::from_be_bytes([buf[0], buf[1]]);
        let d2 = u16::from_be_bytes([buf[2], buf[3]]);
        let d3 = u16::from_be_bytes([buf[4], buf[5]]);

        Some(PrimaryHeader {
            version: (d1 >> 13 & 0x7) as u8,
            type_flag: (d1 >> 12 & 0x1) as u8,
            has_secondary_header: (d1 >> 11 & 0x1) == 1,
            apid: (d1 & 0x7ff),
            sequence_flags: (d2 >> 14 & 0x3) as u8,
            sequence_id: (d2 & 0x3fff),
            len_minus1: d3,
        })
    }
}

/// Calculate the number of missing sequence ids.
///
/// `cur` is the current sequence id. `last` is the sequence id seen before `cur`.
#[must_use]
pub fn missing_packets(cur: u16, last: u16) -> u16 {
    let expected = if last + 1 > PrimaryHeader::SEQ_MAX {
        0
    } else {
        last + 1
    };
    if cur != expected {
        if last + 1 > cur {
            return cur + PrimaryHeader::SEQ_MAX - last;
        }
        return cur - last - 1;
    }
    0
}

pub struct PacketReaderIter<R>
where
    R: Read + Send,
{
    pub reader: R,
    pub offset: usize,
}

impl<R> PacketReaderIter<R>
where
    R: Read + Send,
{
    fn new(reader: R) -> Self {
        PacketReaderIter { reader, offset: 0 }
    }
}

impl<R> Iterator for PacketReaderIter<R>
where
    R: Read + Send,
{
    type Item = IOResult<Packet>;

    fn next(&mut self) -> Option<Self::Item> {
        match Packet::read(&mut self.reader) {
            Ok(mut p) => {
                p.offset = self.offset;
                self.offset += PrimaryHeader::LEN + p.header.len_minus1 as usize + 1;
                Some(Ok(p))
            }
            Err(err) => {
                if err.kind() == std::io::ErrorKind::UnexpectedEof {
                    return None;
                }
                Some(Err(err))
            }
        }
    }
}

/// Packet data representing a CCSDS packet group according to the packet
/// sequencing value in primary header.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PacketGroup {
    pub apid: Apid,
    pub packets: Vec<Packet>,
}

impl PacketGroup {
    /// Return true if this packet group is complete.
    ///
    /// Valid means at least 1 packet and all the packets for a complete group with no missing
    /// packets.
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn complete(&self) -> bool {
        if self.packets.is_empty() {
            false
        } else if self.packets.len() == 1 {
            self.packets[0].is_standalone()
        } else {
            self.packets[0].is_first()
                && self.packets.last().unwrap().is_last()
                && self.have_missing()
        }
    }

    #[must_use]
    pub fn have_missing(&self) -> bool {
        for (a, b) in self.packets.iter().zip(self.packets[1..].iter()) {
            if missing_packets(a.header.sequence_id, b.header.sequence_id) > 0 {
                return true;
            }
        }
        false
    }
}

struct PacketGroupIter<I>
where
    I: Iterator<Item = Packet> + Send,
{
    packets: I,
    cached: Option<Packet>,
    done: bool,
}

impl<I> PacketGroupIter<I>
where
    I: Iterator<Item = Packet> + Send,
{
    /// Create an iterator that reads source packets from the provided reader.
    // fn with_reader<R>(reader: R) -> Self where R: Read + Send {
    //     let packets = PacketReaderIter::new(reader)
    //         .filter(Result::is_ok)
    //         .map(Result::unwrap);
    //     Self::with_packets(packets)
    // }

    /// Create an iterator that sources packets directly from the provided vanilla
    /// iterator.
    ///
    /// Results genreated by the iterator will always be `Ok`.
    fn with_packets(packets: I) -> Self {
        PacketGroupIter {
            packets,
            cached: None,
            done: false,
        }
    }
}

impl<I> Iterator for PacketGroupIter<I>
where
    I: Iterator<Item = Packet> + Send,
{
    type Item = IOResult<PacketGroup>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            // only happens when we finish with a packet left in the cache
            return None;
        }

        let mut group: Option<PacketGroup> = None;
        loop {
            // Get packet from cache first, then try iter
            let packet = match self.cached.take() {
                Some(packet) => packet,
                None => match self.packets.next() {
                    Some(packet) => packet,
                    None => {
                        // nothing cached and iter is done
                        break;
                    }
                },
            };

            group = match group.take() {
                None => {
                    // standalone packet with no current group, just return it
                    if packet.is_standalone() {
                        return Some(Ok(PacketGroup {
                            apid: packet.header.apid,
                            packets: vec![packet],
                        }));
                    }
                    // start a new group with our packet
                    Some(PacketGroup {
                        apid: packet.header.apid,
                        packets: vec![packet],
                    })
                }
                Some(mut group) => {
                    // Different apids indicate we're done with this group. However we have a
                    // packet, so we must cache it for use on next iter.
                    if packet.header.apid != group.packets[0].header.apid {
                        self.cached = Some(packet);
                        return Some(Ok(group));
                    }
                    // Adding to group we already started
                    group.packets.push(packet);
                    Some(group)
                }
            };
        }

        // If we have one, return it.
        if let Some(group) = group {
            return Some(Ok(group));
        }

        // Clear cache
        self.done = true;
        match self.cached.take() {
            Some(packet) => Some(Ok(PacketGroup {
                apid: packet.header.apid,
                packets: vec![packet],
            })),
            None => None,
        }
    }
}

/// Return an iterator providing [Packet] data read from a byte synchronized ungrouped
/// packet stream.
///
/// For packet streams that may contain packets that utilize packet grouping see
/// ``read_packet_groups``.
///
/// # Examples
/// ```
/// use ccsds::read_packets;
///
/// let dat: &[u8] = &[
///     // primary header bytes
///     0xd, 0x59, 0xd2, 0xab, 0x0, 07,
///     // CDS timecode bytes in secondary header
///     0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
///     // minimum 1 byte of user data
///     0xff
/// ];
///
/// let r = std::io::BufReader::new(dat);
/// read_packets(r).for_each(|zult| {
///     let packet = zult.unwrap();
///     assert_eq!(packet.header.apid, 1369);
/// });
/// ```
pub fn read_packets<R>(reader: R) -> impl Iterator<Item = IOResult<Packet>> + Send
where
    R: Read + Send,
{
    PacketReaderIter::new(reader)
}

/// Return an [Iterator] that groups read packets into ``PacketGroup``s.
///
/// This is necessary for packet streams containing APIDs that utilize packet grouping sequence
/// flags values ``SEQ_FIRST``, ``SEQ_CONTINUATION``, and ``SEQ_LAST``. It can also be used for
/// non-grouped APIDs (``SEQ_UNSEGMENTED``), however, it is not necessary in such cases. See
/// ``PrimaryHeader::sequence_flags``.
///
/// # Examples
///
/// Reading packets from data file of space packets (level-0) would look something
/// like this:
/// ```
/// use ccsds::read_packet_groups;
///
/// // data file stand-in
/// let dat: &[u8] = &[
///     // primary header bytes
///     0xd, 0x59, 0xd2, 0xab, 0x0, 07,
///     // CDS timecode bytes in secondary header
///     0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
///     // minimum 1 byte of user data
///     0xff
/// ];
///
/// let r = std::io::BufReader::new(dat);
/// read_packet_groups(r).for_each(|zult| {
///     let packet = zult.unwrap();
///     assert_eq!(packet.apid, 1369);
/// });
/// ```
pub fn read_packet_groups<R>(reader: R) -> impl Iterator<Item = IOResult<PacketGroup>>
where
    R: Read + Send,
{
    let packets = PacketReaderIter::new(reader).flatten();
    PacketGroupIter::with_packets(packets)
}

/// Collects the provided packets into ``PacketGroup``s.
pub fn collect_packet_groups<I>(packets: I) -> impl Iterator<Item = IOResult<PacketGroup>> + Send
where
    I: Iterator<Item = Packet> + Send,
{
    PacketGroupIter::with_packets(packets)
}

struct VcidTracker {
    vcid: VCID,
    /// Caches partial packets for this vcid
    cache: Vec<u8>,
    // True when any frame used to fill the cache was rs corrected
    rs_corrected: bool,
    // True when a FHP has been found and data should be added to cache. False
    // where there is a missing data due to RS failure or missing frames.
    sync: bool,
}

impl VcidTracker {
    fn new(vcid: VCID) -> Self {
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

#[derive(Debug, Clone)]
pub struct DecodedPacket {
    pub scid: SCID,
    pub vcid: VCID,
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
    cache: HashMap<VCID, VcidTracker>,
    // Packets that have already been decoded and are waiting to be provided.
    ready: VecDeque<DecodedPacket>,
}

impl<I> Iterator for FramedPacketIter<I>
where
    I: Iterator<Item = DecodedFrame> + Send,
{
    type Item = DecodedPacket;

    fn next(&mut self) -> Option<Self::Item> {
        use rs2::RSState::{Corrected, Uncorrectable};

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
                rsstate,
            } = frame.unwrap();
            let mpdu = frame.mpdu(self.izone_length, self.trailer_length).unwrap();
            let tracker = self
                .cache
                .entry(frame.header.vcid)
                .or_insert(VcidTracker::new(frame.header.vcid));

            if let Corrected(num) = rsstate {
                debug!(vcid = %frame.header.vcid, bytes_corrected=num, "corrected frame");
                tracker.rs_corrected = true;
            }

            // Data loss means we dump what we're working on and force resync
            if let Uncorrectable(_) = rsstate {
                debug!(vcid = %frame.header.vcid, tracker = %tracker, "uncorrectable frame, dropping tracker");
                tracker.clear();
                tracker.sync = false;
                continue;
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
            let mut header = PrimaryHeader::decode(&tracker.cache).unwrap();
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
                        header: PrimaryHeader::decode(data)?,
                        data: data.to_vec(),
                        offset: 0,
                    },
                };
                tracker.cache = tail.to_vec();
                self.ready.push_back(packet);

                if tracker.cache.len() < PrimaryHeader::LEN {
                    break;
                }
                header = PrimaryHeader::decode(&tracker.cache).unwrap();
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
        frames: frames.filter(move |dc| !dc.frame.is_fill()),
        izone_length,
        trailer_length,
        cache: HashMap::new(),
        ready: VecDeque::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_packet() {
        let dat: [u8; 15] = [
            // Primary/secondary header and a single byte of user data
            0xd, 0x59, 0xd2, 0xab, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];
        let x = &dat[..];
        let mut r = std::io::BufReader::new(x);
        let packet = Packet::read(&mut r).unwrap();

        assert_eq!(packet.header.version, 0);
    }

    #[test]
    fn test_read_header() {
        let dat: [u8; 6] = [
            // bytes from a SNPP CrIS packet
            0xd, 0x59, 0xd2, 0xab, 0xa, 0x8f,
        ];
        let x = &dat[0..];
        let mut r = std::io::BufReader::new(x);
        let ph = PrimaryHeader::read(&mut r).unwrap();

        assert_eq!(ph.version, 0);
        assert_eq!(ph.type_flag, 0);
        assert!(ph.has_secondary_header);
        assert_eq!(ph.apid, 1369);
        assert_eq!(ph.sequence_flags, 3);
        assert_eq!(ph.sequence_id, 4779);
        assert_eq!(ph.len_minus1, 2703);
    }

    #[test]
    fn packet_iter_test() {
        #[rustfmt::skip]
        let dat: &[u8] = &[
            // Primary/secondary header and a single byte of user data
            // byte 4 is sequence number 1 & 2
            0xd, 0x59, 0xc0, 0x01, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
            0xd, 0x59, 0xc0, 0x02, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];
        let reader = std::io::BufReader::new(dat);

        let packets: Vec<Packet> = PacketReaderIter::new(reader)
            .filter_map(Result::ok)
            .collect();

        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].header.apid, 1369);
        assert_eq!(packets[0].header.sequence_id, 1);
        assert_eq!(&packets[0].data[..], &dat[..15]);
        assert_eq!(packets[1].header.sequence_id, 2);
        assert_eq!(&packets[1].data[..], &dat[15..]);
    }

    #[test]
    fn test_missing_packets() {
        assert_eq!(missing_packets(5, 4), 0);
        assert_eq!(missing_packets(5, 3), 1);
        assert_eq!(missing_packets(0, PrimaryHeader::SEQ_MAX), 0);
        assert_eq!(missing_packets(0, PrimaryHeader::SEQ_MAX - 1), 1);
        assert_eq!(missing_packets(0, 0), PrimaryHeader::SEQ_MAX);
    }
}
