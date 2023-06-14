use serde::{Serialize, Serializer};
use std::error::Error;
use std::io::Read;
use std::iter::Iterator;
use std::{cmp, collections::HashMap};

use chrono::{DateTime, TimeZone, Utc};

use super::timecode::TimecodeParser;
use super::{Packet, PrimaryHeader, SEQ_FIRST, SEQ_STANDALONE};

const MAX_SEQ_NUM: i32 = 16383;

/// Stream provides the ability to iterate of a reader to provided its
/// contained packet sequence.
pub struct Stream<'a> {
    reader: &'a mut dyn Read,
    err: Option<Box<dyn Error>>,
}

impl<'a> Stream<'a> {
    pub fn new(reader: &mut dyn Read) -> Stream {
        Stream { reader, err: None }
    }
}

impl<'a> Iterator for Stream<'a> {
    type Item = Packet;

    fn next(&mut self) -> Option<Self::Item> {
        match Packet::read(&mut self.reader) {
            Ok(p) => {
                return Some(p);
            }
            Err(err) => {
                self.err = Some(err);
                None
            }
        }
    }
}

pub(crate) fn serialize_dt<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    dt.format("%Y-%m-%dT%H:%M:%S.%fZ")
        .to_string()
        .serialize(serializer)
}

pub(crate) fn serialize_err<S>(
    err: &Option<Box<dyn Error>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if let Some(err) = err {
        err.to_string().serialize(serializer)
    } else {
        serializer.serialize_none()
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
    err: Option<Box<dyn Error>>,
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
            tc_parser: tc_parser,
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
                total_bytes: total_bytes,
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

fn collect_groups() {}

#[cfg(test)]
mod tests {
    use crate::spacepacket::parse_cds_timecode;

    use super::*;
    use std::io::BufReader;

    #[test]
    fn stream_test() {
        #[rustfmt::skip]
        let dat: &[u8] = &[
            // Primary/secondary header and a single byte of user data
            // byte 4 is sequence number 1 & 2
            0xd, 0x59, 0xc0, 0x01, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
            0xd, 0x59, 0xc0, 0x02, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];
        let mut reader = BufReader::new(dat);
        let stream = Stream::new(&mut reader);

        let packets: Vec<Packet> = stream.collect();

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
        let mut reader = BufReader::new(dat);
        let stream = Stream::new(&mut reader);
        let mut summarizer = Summarizer::new(&parse_cds_timecode);
        for packet in stream {
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
