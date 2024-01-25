use std::collections::VecDeque;
use std::fmt::Display;
use std::io::{Read, Result as IOResult};
use std::{collections::HashMap, convert::TryInto};

pub use crate::timecode::{
    decode_cds_timecode, decode_eoscuc_timecode, CDSTimecode, EOSCUCTimecode, Timecode,
    TimecodeParser, Error as TimecodeError,
};
use crate::{DecodedFrame, SCID, VCID};
use serde::{Deserialize, Serialize};

/// Maximum packet sequence id before rollover.
pub const MAX_SEQ_NUM: i32 = 16383;

pub type APID = u16;

/// Packet represents a single CCSDS space packet and its associated data.
///
/// This packet contains the primary header data as well as the user data,
/// which may or may not container a secondary header. See the header's
/// `has_secondary_header` flag.
///
/// # Example
/// Create a packet from the minimum number of bytes. This example includes
/// bytes for a `CDSTimecode` in the data zone.
/// ```
/// use ccsds::{
///     CDSTimecode,
///     decode_cds_timecode,
///     Packet,
///     PrimaryHeader,
/// };
///
/// let dat: &[u8] = &[
///     // primary header bytes
///     0xd, 0x59, 0xd2, 0xab, 0x0, 07,
///     // CDS timecode bytes in secondary header
///     0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
///     // minimum 1 byte of user data
///     0xff
/// ];
/// let mut r = std::io::BufReader::new(dat);
/// let packet = Packet::read(&mut r).unwrap();
/// let tc = decode_cds_timecode(&packet.data[PrimaryHeader::LEN..]);
/// ```
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Packet {
    /// All packets have a primary header
    pub header: PrimaryHeader,
    /// All packet bytes, including header and user data
    pub data: Vec<u8>,
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
    pub fn is_first(&self) -> bool {
        self.header.sequence_flags == SEQ_FIRST
    }

    pub fn is_last(&self) -> bool {
        self.header.sequence_flags == SEQ_LAST
    }

    pub fn is_cont(&self) -> bool {
        self.header.sequence_flags == SEQ_CONTINUATION
    }

    pub fn is_standalone(&self) -> bool {
        self.header.sequence_flags == SEQ_UNSEGMENTED
    }

    /// Decode from bytes. Returns `None` if there are not enough bytes to construct the
    /// header or if there are not enough bytes to construct the [Packet] of the length
    /// indicated by the header.
    pub fn decode(dat: &mut [u8]) -> Option<Packet> {
        match PrimaryHeader::decode(dat) {
            Some(header) => {
                if dat.len() < header.len_minus1 as usize + 1 {
                    None
                } else {
                    Some(Packet {
                        header,
                        data: dat.to_vec(),
                    })
                }
            }
            None => None,
        }
    }

    /// Read a single [Packet].
    pub fn read(r: &mut dyn Read) -> IOResult<Packet> {
        let ph = PrimaryHeader::read(r)?;

        // read the user data, shouldn't panic since unpacking worked
        let mut buf = vec![0u8; (ph.len_minus1 + 1).try_into().unwrap()];

        r.read_exact(&mut buf)?;

        Ok(Packet {
            header: ph,
            data: buf,
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
    pub apid: APID,
    /// Defines a packets grouping. See the `SEQ_*` values.
    pub sequence_flags: u8,
    pub sequence_id: u16,
    pub len_minus1: u16,
}

impl PrimaryHeader {
    /// Size of a PrimaryHeader
    pub const LEN: usize = 6;

    pub fn read(r: &mut dyn Read) -> IOResult<PrimaryHeader> {
        let mut buf = [0u8; Self::LEN];
        r.read_exact(&mut buf)?;

        Ok(Self::decode(&buf).unwrap())
    }

    /// Decode from bytes. Returns `None` if there are not enough bytes to construct the
    /// header.
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
            apid: (d1 & 0x7ff) as u16,
            sequence_flags: (d2 >> 14 & 0x3) as u8,
            sequence_id: (d2 & 0x3fff) as u16,
            len_minus1: d3,
        })
    }
}

struct PacketReaderIter<'a> {
    reader: &'a mut dyn Read,
    offset: usize,
}

impl<'a> PacketReaderIter<'a> {
    fn new(reader: &'a mut dyn Read) -> Self {
        PacketReaderIter { reader, offset: 0 }
    }

    // fn _check_missing(&self, packet: &Packet) -> Option<u16> {
    //     match self.last.get(&packet.header.apid) {
    //         Some(prev_seq) => {
    //             let cur_seq = packet.header.apid as i32;
    //             let prev_seq = *prev_seq as i32;
    //             let expected = (prev_seq + 1) % (MAX_SEQ_NUM + 1);
    //             if cur_seq != expected {
    //                 return Some((cur_seq - prev_seq - 1) as u16);
    //             }
    //             return None;
    //         }
    //         None => None,
    //     }
    // }
}

impl<'a> Iterator for PacketReaderIter<'a> {
    type Item = IOResult<Packet>;

    fn next(&mut self) -> Option<Self::Item> {
        match Packet::read(&mut self.reader) {
            Ok(p) => {
                self.offset += PrimaryHeader::LEN + p.header.len_minus1 as usize + 1;
                return Some(Ok(p));
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

type PacketIter<'a> = Box<dyn Iterator<Item = IOResult<Packet>> + 'a>;

/// Packet data representing a CCSDS packet group according to the packet
/// sequencing value in primary header.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PacketGroup {
    pub apid: APID,
    pub packets: Vec<Packet>,
}

struct PacketGroupIter<'a> {
    packets: PacketIter<'a>,
    group: PacketGroup,
    done: bool,
}

impl<'a> PacketGroupIter<'a> {
    /// Create an iterator that reads source packets from the provided reader.
    ///
    ///
    fn with_reader(reader: &'a mut dyn Read) -> Self {
        let packets = PacketReaderIter::new(reader)
            .filter(|zult| zult.is_ok())
            .map(|zult| zult.unwrap());
        Self::with_packets(Box::new(packets))
    }

    /// Create an iterator that sources packets directly from the provided vanilla
    /// iterator.
    ///
    /// Results genreated by the iterator will always be `Ok`.
    fn with_packets(packets: Box<dyn Iterator<Item = Packet> + 'a>) -> Self {
        let packets: PacketIter = Box::new(packets.map(|p| IOResult::<Packet>::Ok(p)));
        PacketGroupIter {
            packets,
            group: PacketGroup {
                apid: 0,
                packets: vec![],
            },
            done: false,
        }
    }

    /// True when this group contains at least 1 packet.
    fn have_packets(&self) -> bool {
        self.group.packets.len() > 0
    }

    /// Given our current state, does packet indicate we should start a new group.
    fn should_start_new_group(&self, packet: &Packet) -> bool {
        packet.is_first()
            || (self.group.packets.len() > 0
                && self.group.packets[0].header.apid != packet.header.apid)
    }

    /// Create a new group, returning the old, priming it with the packet
    fn new_group(&mut self, packet: Option<Packet>) -> PacketGroup {
        let group = self.group.clone();
        self.group = PacketGroup {
            apid: 0,
            packets: vec![],
        };
        if let Some(p) = packet {
            self.group.apid = p.header.apid;
            self.group.packets.push(p);
        }
        group
    }
}

impl Iterator for PacketGroupIter<'_> {
    type Item = IOResult<PacketGroup>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        'outer: loop {
            // Get a packet from the iterator. Exit the iterator
            let packet: Packet = match self.packets.next() {
                Some(zult) => {
                    match zult {
                        // Got a packet from the packet iter
                        Ok(packet) => packet,
                        // Got an error from the packet iter. Return a result with the
                        // error to let the consumer decide what to do.
                        Err(err) => return Some(Err(err)),
                    }
                }
                None => break 'outer,
            };
            // Return group of one
            if packet.is_standalone() {
                return Some(Ok(PacketGroup {
                    apid: packet.header.apid,
                    packets: vec![packet],
                }));
            }
            if self.should_start_new_group(&packet) {
                if self.have_packets() {
                    return Some(Ok(self.new_group(Some(packet))));
                }
                self.new_group(None);
            }
            self.group.packets.push(packet);
        }

        self.done = true;
        // We're all done, so return any partial group
        if self.have_packets() {
            return Some(Ok(self.new_group(None)));
        }
        return None;
    }
}

/// Return an iterator providing [Packet] data read from a byte synchronized ungrouped
/// packet stream.
///
/// For packet streams that may contain packets that utilize packet grouping see
/// [read_packet_groups].
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
/// let mut r = std::io::BufReader::new(dat);
/// read_packets(&mut r).for_each(|zult| {
///     let packet = zult.unwrap();
///     assert_eq!(packet.header.apid, 1369);
/// });
/// ```
pub fn read_packets<'a>(reader: &'a mut dyn Read) -> impl Iterator<Item = IOResult<Packet>> + 'a {
    PacketReaderIter::new(reader)
}

/// Return an [Iterator] that groups read packets into [PacketGroup]s.
///
/// This is necessary for packet streams containing APIDs that utilize packet grouping sequence
/// flags values [SEQ_FIRST], [SEQ_CONTINUATION], and [SEQ_LAST]. It can also be used for
/// non-grouped APIDs ([SEQ_UNSEGMENTED]), however, it is not necessary in such cases. See
/// [PrimaryHeader::sequence_flags].
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
/// let mut r = std::io::BufReader::new(dat);
/// read_packet_groups(&mut r).for_each(|zult| {
///     let packet = zult.unwrap();
///     assert_eq!(packet.apid, 1369);
/// });
/// ```
pub fn read_packet_groups<'a>(
    reader: &'a mut dyn Read,
) -> impl Iterator<Item = IOResult<PacketGroup>> + 'a {
    PacketGroupIter::with_reader(reader)
}

/// Collects the provided packets into [PacketGroup]s.
pub fn collect_packet_groups<'a>(
    packets: Box<dyn Iterator<Item = Packet> + 'a>,
) -> impl Iterator<Item = IOResult<PacketGroup>> + 'a {
    PacketGroupIter::with_packets(packets)
}

struct VcidTracker {
    cache: Vec<u8>,
    rs_corrected: bool,
}

struct FramedPacketIter<'a> {
    frames: Box<dyn Iterator<Item = DecodedFrame> + 'a>,
    izone_length: usize,
    trailer_length: usize,

    // True when a FHP has been found and data should be added to cache. False
    // where there is a missing data due to RS failure or missing frames.
    sync: bool,
    // Cache of partial packet data from frames that has not yet been decoded into
    // packets. There should only be up to about 1 frame worth of data in the cache
    // per scid/vcid.
    cache: HashMap<(SCID, VCID), VcidTracker>,
    // Packets that have already been decoded and are waiting to be provided.
    ready: VecDeque<Packet>,
}

impl<'a> Iterator for FramedPacketIter<'a> {
    type Item = Packet;

    fn next(&mut self) -> Option<Self::Item> {
        use rs2::RSState::*;

        // If there are ready packets provide the oldest one
        if let Some(packet) = self.ready.pop_front() {
            return Some(packet);
        }

        // No packet ready, we have to find one
        loop {
            let frame = self.frames.next();
            if frame.is_none() {
                break;
            }

            let DecodedFrame {
                frame,
                missing,
                rsstate,
            } = frame.unwrap();

            // If frame is fill, so is the MPDU
            if frame.is_fill() {
                continue;
            }

            let mpdu = frame.mpdu(self.izone_length, self.trailer_length);

            // Data loss means we dump what we're working on and resync
            if let Uncorrectable(_) = rsstate {
                self.sync = false;
                continue;
            }

            let key = (frame.header.scid, frame.header.vcid);
            // A frame counter error only indicates that there are missing frames before this
            // frame, however, this frame's data can still be used. The current vcid packet is now
            // trash and we lose sync, but we do not have throw out the current frame so we let
            // processing continue.
            if missing.is_some() {
                let _ = self.cache.remove(&key);
                self.sync = false;
            }

            let tracker = self.cache.entry(key).or_insert(VcidTracker {
                cache: vec![],
                rs_corrected: false,
            });

            if self.sync {
                // If we have sync add the VCID data to its cache
                tracker.cache.extend_from_slice(mpdu.payload());
            } else {
                // No way to get sync if we don't have a header
                if !mpdu.has_header() {
                    continue;
                }
                tracker.cache = mpdu.payload()[mpdu.header_offset()..].to_vec();
                if let Corrected(_) = rsstate {
                    tracker.rs_corrected = true
                }
                self.sync = true;
            }

            // Collect all packets found in this frame/mpdu until we there's not
            // enough data.
            loop {
                if tracker.cache.len() < PrimaryHeader::LEN {
                    break;
                }

                // Construct the header w/o consuming the bytes
                let header = PrimaryHeader::decode(&tracker.cache).unwrap();
                let need = header.len_minus1 as usize + 1 + PrimaryHeader::LEN;
                if tracker.cache.len() < need {
                    break;
                }

                // Grab data we need and update the cache
                let (data, tail) = tracker.cache.split_at(need);
                let packet = Packet {
                    header,
                    data: data.to_vec(),
                };
                tracker.cache = tail.to_vec();

                self.ready.push_back(packet);
            }

            // Decoding all done, provide what we found
            return self.ready.pop_front();
        }

        // Attempted to read a frame, but the iterator is done.  Make sure to
        // provide a ready frame if there are any.
        return self.ready.pop_front();
    }
}

/// Decodes the provided frames into a packets contained within the frames' MPDUs.
///
/// There are several cases when frame data cannot be fully recovered and is dropped,
/// i.e., not used to construct packets:
///
/// 1. Missing frames
/// 2. Frames with state [rs2::RSState::Uncorrectable]
/// 3. Fill Frames
/// 4. Frames before the first header is available in an MPDU
///
/// This will handle frames from multiple spacecrafts, i.e., with different SCIDs.
pub fn decode_framed_packets<'a>(
    frames: Box<dyn Iterator<Item = DecodedFrame> + 'a>,
    izone_length: usize,
    trailer_length: usize,
) -> impl Iterator<Item = Packet> + 'a {
    FramedPacketIter {
        frames,
        izone_length,
        trailer_length,
        sync: false,
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
        assert_eq!(ph.has_secondary_header, true);
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
        let mut reader = std::io::BufReader::new(dat);

        let packets: Vec<Packet> = PacketReaderIter::new(&mut reader)
            .filter(|r| r.is_ok())
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].header.apid, 1369);
        assert_eq!(packets[0].header.sequence_id, 1);
        assert_eq!(packets[1].header.sequence_id, 2);
    }
}
