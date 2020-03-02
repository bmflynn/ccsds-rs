use std::error::Error;
use std::io::Read;
use std::iter::Iterator;

use crate::error::DecodeError;
use crate::packet::Packet;

/// Stream provides the ability to iterate of a reader to provided its
/// contained packet sequence.
pub struct Stream {
    reader: Box<dyn Read>,
    err: Option<Box<dyn Error>>,
}

impl Stream {
    pub fn new(reader: Box<dyn Read>) -> Stream {
        Stream {
            reader: reader,
            err: None,
        }
    }
}

impl Iterator for Stream {
    type Item = Result<Packet, DecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        match Packet::read(&mut self.reader) {
            Ok(p) => Some(Ok(p)),
            Err(err) => {
                self.err = Some(err);
                None
            }
        }
    }
}

/*
 * FIXME: Sequencer's impl is flawed b/c it does not play well with borrow system.
 *
 * The iter requires a &mut self ref which means the instance cannot be used after
 * iteration.
 *
 * TODO: Consider a providing a method to return a wrapped iter that will compute
 *       the gaps while iterating, afterwhich the zults can be retreived.
 *
const MAX_SEQ_NUM: u16 = 16383;

#[derive(Debug)]
pub struct Gap {
    // max gap size due to sequence counter rollover
    pub count: u16,
    // starting sequence id
    pub start: u16,
    // byte offset into reader where last packet before the gap
    pub offset: u64,
}

/// Sequencer is an adapter for a `Stream` that keeps track of packet sequence
/// data. Works as a drop in replacement for `Stream`.
pub struct Sequencer {
    stream: Stream,
    offset: u64,
    // apid -> last seen packet
    tracker: Box<HashMap<u16, PrimaryHeader>>,
    gaps: Box<Vec<Gap>>,
}

impl Sequencer {
    pub fn new(stream: Stream) -> Sequencer {
        return Sequencer {
            stream: stream,
            offset: 0,
            tracker: Box::new(HashMap::new()),
            gaps: Box::new(Vec::new()),
        };
    }

    pub fn gaps(&self) -> &[Gap] {
        self.gaps.as_slice()
    }

    fn handle_sequence(&mut self, packet: &Packet) {
        let hdr = packet.header.clone();
        let apid = packet.header.apid.clone();
        let seq: i32 = packet.header.sequence_id.clone() as i32;

        if let Some(prev_hdr) = self.tracker.get(&apid) {
            let prev_seq: i32= prev_hdr.sequence_id as i32;

            // check if sequence numbers are sequential w/ rollover
            if (prev_seq + 1) % MAX_SEQ_NUM as i32 != seq {
                self.gaps.push(Gap {
                    count: ((seq - prev_seq) % (MAX_SEQ_NUM as i32)) as u16,
                    start: prev_seq as u16,
                    offset: self.offset.clone(),
                });
            }
        };

        // record current as last packet seen
        self.tracker.insert(apid, hdr);
    }
}

impl Iterator for Sequencer {
    type Item = Result<Packet, DecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.stream.next() {
            Some(zult) => match zult {
                Ok(p) => {
                    self.offset += (PrimaryHeader::SIZE + p.data.len()) as u64;
                    self.handle_sequence(&p);
                    Some(Ok(p))
                }
                Err(err) => Some(Err(err)),
            },
            None => None,
        }
    }
}

*/

#[cfg(test)]
mod tests {
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
        let reader = BufReader::new(dat);
        let stream = Stream::new(Box::new(reader));

        let packets: Vec<Packet> = stream
            .filter(|zult| zult.is_ok())
            .map(|zult| zult.unwrap())
            .collect();

        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].header.apid, 1369);
        assert_eq!(packets[0].header.sequence_id, 1);
        assert_eq!(packets[1].header.sequence_id, 2);
    }

    /*
    #[test]
    fn sequencer_test() {
        #[rustfmt::skip]
        let dat: &[u8] = &[
            // Primary/secondary header and a single byte of user data
            // byte 4 is sequence number 1 & 2
            0xd, 0x59, 0xc0, 0x01, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
            0xd, 0x59, 0xc0, 0x03, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
            0xd, 0x59, 0xc0, 0x05, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
            0xd, 0x59, 0xc0, 0x06, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];
        let reader = BufReader::new(dat);
        let stream = Stream::new(Box::new(reader));
        let sequencer = Sequencer::new(stream);

        let packets: Vec<Result<Packet, DecodeError>> = sequencer.collect();

        let gaps = sequencer.gaps();

        assert!(false, "FIXME: sequencer not generating expected gaps");

        assert_eq!(gaps.len(), 2, "expected 2 gaps");
        assert_eq!(gaps[0].offset, 2, "{:?}", gaps[0]);

    }
    */
}
