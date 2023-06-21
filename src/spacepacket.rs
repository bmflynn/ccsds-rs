mod timecode;

use std::cmp;
use std::collections::HashMap;
use std::fmt::Display;
use std::io::Read;
use std::convert::TryInto;
use chrono::{DateTime, TimeZone, Utc};

pub use timecode::{
    Timecode,
    TimecodeParser,
    CDSTimecode,
    EOSCUCTimecode,
    parse_cds_timecode,
    parse_eoscuc_timecode,
};

const MAX_SEQ_NUM: i32 = 16383;

pub type APID = u16;

/// Packet represents a single CCSDS space packet and its associated data.
///
/// This packet contains the primary header data as well as the user data,
/// which may or may not container a secondary header. See the header's
/// `has_secondary_header` flag.
///
/// # Example
/// Create a packet from the minumum number of bytes. This example includes
/// bytes for a `CDSTimecode` in the data zone.
/// ```
/// use ccsds::spacepacket::{
///     CDSTimecode,
///     parse_cds_timecode,
///     Packet,
///     PrimaryHeader,
/// };
///
/// let dat: &[u8] = &[
///     // primary header bytes
///     0xd, 0x59, 0xd2, 0xab, 0x0, 0x7,
///     // CDS timecode bytes in secondary header
///     0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
///     // minimum 1 byte of user data
///     0xff
/// ];
/// let mut r = std::io::BufReader::new(dat);
/// let packet = Packet::read(&mut r).unwrap();
/// let tc = parse_cds_timecode(&packet.data[PrimaryHeader::SIZE..]);
/// ```
#[derive(Debug, Clone)]
pub struct Packet {
    /// All packets have a primary header
    pub header: PrimaryHeader,
    /// User data, which may include a secondary header
    pub data: Vec<u8>,
}

impl Display for Packet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Packet{{header: {:?}, data:[len={}]}}", self.header, self.data.len())?;
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
        self.header.sequence_flags == SEQ_CONT
    }

    pub fn is_standalone(&self) -> bool {
        self.header.sequence_flags == SEQ_STANDALONE
    }

    pub fn read(r: &mut dyn Read) -> Result<Packet, std::io::Error> {
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

pub const SEQ_FIRST: u8 = 1;
pub const SEQ_CONT: u8 = 0;
pub const SEQ_LAST: u8 = 2;
pub const SEQ_STANDALONE: u8 = 3;

/// CCSDS Primary Header
///
/// The primary header format is common to all CCSDS space packets.
///
#[derive(Debug, Copy, Clone)]
pub struct PrimaryHeader {
    pub version: u8,
    pub type_flag: u8,
    pub has_secondary_header: bool,
    pub apid: u16,
    pub sequence_flags: u8,
    pub sequence_id: u16,
    pub len_minus1: u16,
}

impl PrimaryHeader {
    /// Size of a PrimaryHeader
    pub const SIZE: usize = 6;

    pub fn read(r: &mut dyn Read) -> Result<PrimaryHeader, std::io::Error> {
        let mut buf = [0u8; Self::SIZE];
        r.read_exact(&mut buf)?;

        let d1 = u16::from_be_bytes([buf[0], buf[1]]);
        let d2 = u16::from_be_bytes([buf[2], buf[3]]);
        let d3 = u16::from_be_bytes([buf[4], buf[5]]);

        Ok(PrimaryHeader {
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

pub struct PacketIter<'a> {
    reader: &'a mut dyn Read,
}

impl <'a> PacketIter<'a> {
    pub fn new(reader: &'a mut dyn Read) -> Self { 
        PacketIter{reader}
    }
}

impl<'a> Iterator for PacketIter<'a> {
    type Item = Result<Packet, std::io::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match Packet::read(&mut self.reader) {
            Ok(p) => {
                return Some(Ok(p))
            }
            Err(err) => {
                if err.kind() == std::io::ErrorKind::UnexpectedEof {
                    return None
                }
                Some(Err(err))
            }
        }
    }
}

#[derive(Clone)]
pub struct Group {
    pub packets: Vec<Packet>,
}

pub struct GroupIter<'a> {
    packets: PacketIter<'a>,
    group: Group,
    done: bool,
}

impl <'a> GroupIter<'a> {
    pub fn new(reader: &'a mut dyn Read) -> Self {
        let packets = PacketIter::new(reader);
        GroupIter{packets, group: Group{packets: vec![]}, done: false}
    }
}

impl<'a> GroupIter<'a> {
    fn have_packets(&self) -> bool {
        self.group.packets.len() > 0
    }

    fn should_start_new_group(&self, packet: &Packet) -> bool {
        packet.is_first() ||
            (self.group.packets.len() > 0 && self.group.packets[0].header.apid != packet.header.apid)
   }

    /// Create a new group, returning the old, priming it with the packet
    fn new_group(&mut self, packet: Option<Packet>) -> Group {
        let group = self.group.clone();
        self.group = Group{packets: vec![]};
        if let Some(p) = packet {
            self.group.packets.push(p);
        }
        group
    }
}

impl<'a> Iterator for GroupIter<'a> {
    type Item = Result<Group, std::io::Error>;

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
                },
                None => break 'outer,
            };
            // Return group of one
            if packet.is_standalone() {
                return Some(Ok(Group{packets: vec![packet]})); 
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

#[derive(Debug, Clone)]
pub struct Gap {
    // Number of packets in gap. There's no guarantee that rollover did not occur
    // which may occur if the gap is bigger than the max seq number.
    pub count: u16,
    // starting sequence id
    pub start: u16,
    // byte offset into reader for the first byte _after_ the gap.
    pub offset: usize,
}

#[derive(Debug, Clone)]
pub struct ApidInfo {
    total_count: u32,
    total_bytes: usize,
    gaps: Box<Vec<Gap>>,
    first: DateTime<Utc>,
    last: DateTime<Utc>,
}

#[derive(Clone)]
pub struct Summary {
    apids: HashMap<u16, ApidInfo>,
    total_count: u32,
    total_bytes: usize,
    first: DateTime<Utc>,
    last: DateTime<Utc>,
}

pub struct Summarizer<'a> {
    summary: Summary,
    tc_parser: &'a TimecodeParser,
    offset: usize,
    tracker: HashMap<u16, PrimaryHeader>,
    err: Option<std::io::Error>,
}

impl<'a> Summarizer<'a> {
    pub fn new(tc_parser: &'a TimecodeParser) -> Self {
        return Summarizer {
            summary: Summary {
                apids: HashMap::new(),
                total_count: 0,
                total_bytes: 0,
                first: Utc::now(),
                last: Utc.timestamp_opt(0, 0).single().unwrap(),
            },
            tracker: HashMap::new(),
            tc_parser,
            offset: 0,
            err: None,
        };
    }
    pub fn add(&mut self, pkt: &Packet) {
        // can't add more if we've already encountered an error
        if self.err.is_some() {
            return;
        }

        let hdr = pkt.header.clone();
        let seq = pkt.header.sequence_id as i32;
        let total_bytes = PrimaryHeader::SIZE + pkt.data.len();

        self.offset += total_bytes;
        self.summary.total_count += 1;
        self.summary.total_bytes += total_bytes;

        // Handle individual apid information
        let mut apid = match self.summary.apids.remove(&pkt.header.apid) {
            Some(a) => a,
            None => ApidInfo {
                total_count: 1,
                total_bytes,
                gaps: Box::new(Vec::new()),
                first: Utc::now(),
                last: Utc.timestamp_opt(0, 0).single().unwrap(),
            },
        };
        apid.total_count += 1;
        apid.total_bytes += total_bytes;

        // Handle gap checking
        if let Some(prev_hdr) = self.tracker.get(&hdr.apid) {
            let prev_seq = prev_hdr.sequence_id as i32;
            // check if sequence numbers are sequential w/ rollover
            let expected = (prev_seq + 1) % (MAX_SEQ_NUM + 1);
            if seq != expected {
                apid.gaps.push(Gap {
                    count: (seq - prev_seq - 1) as u16,
                    start: prev_seq as u16,
                    // offset already includes packet, so subtract it out.
                    offset: self.offset - (pkt.data.len() + PrimaryHeader::SIZE) as usize,
                });
            }
        };
        self.tracker.insert(hdr.apid, hdr);

        // Handle first/last packet times
        if pkt.header.has_secondary_header
            && (pkt.header.sequence_flags == SEQ_FIRST
                || pkt.header.sequence_flags == SEQ_STANDALONE)
        {
            match (self.tc_parser)(&pkt.data) {
                Ok(dt) => {
                    self.summary.first = cmp::min(self.summary.first, dt.clone());
                    self.summary.last = cmp::max(self.summary.last, dt.clone());
                    apid.first = cmp::min(apid.first, dt.clone());
                    apid.last = cmp::max(apid.last, dt.clone());
                }
                _ => {}
            };
        };

        self.summary.apids.insert(pkt.header.apid, apid);
    }

    pub fn result(&self) -> Summary {
        self.summary.clone()
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

        let packets: Vec<Packet> = PacketIter::new(&mut reader)
            .filter(|r| r.is_ok())
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].header.apid, 1369);
        assert_eq!(packets[0].header.sequence_id, 1);
        assert_eq!(packets[1].header.sequence_id, 2);
    }

    #[test]
    fn summarize_test() {
        #[rustfmt::skip]
        let dat: &[u8] = &[
            // Primary/secondary header and a single byte of user data
            // byte 4 is sequence number 1 & 2
            0xd, 0x59, 0xc0, 0x01, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
            // gap
            0xd, 0x59, 0xc0, 0x03, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
            // gap
            0xd, 0x59, 0xc0, 0x05, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
            0xd, 0x59, 0xc0, 0x06, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];
        let mut reader = std::io::BufReader::new(dat);
        let packets: Vec<Packet> = PacketIter::new(&mut reader)
            .filter(|r| r.is_ok())
            .map(|r| r.unwrap())
            .collect();
        let mut summarizer = Summarizer::new(&parse_cds_timecode);
        for packet in packets {
            summarizer.add(&packet);
        }

        let summary = summarizer.result();
        let mut gaps: Box<Vec<&Gap>> = Box::new(Vec::new());
        for apid in summary.apids.values() {
            for gap in apid.gaps.iter() {
                gaps.push(gap);
            }
        }

        assert_eq!(gaps.len(), 2, "expected 2 gaps");

        assert_eq!(gaps[0].count, 1, "{:?}", gaps[0]);
        assert_eq!(gaps[0].start, 1, "{:?}", gaps[0]);
        assert_eq!(gaps[0].offset, 15, "{:?}", gaps[0]);

        assert_eq!(gaps[1].count, 1, "{:?}", gaps[1]);
        assert_eq!(gaps[1].start, 3, "{:?}", gaps[1]);
        assert_eq!(gaps[1].offset, 30, "{:?}", gaps[1]);
    }
}
