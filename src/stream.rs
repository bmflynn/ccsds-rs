use std::error::Error;
use std::io::Read;
use std::iter::Iterator;

use crate::error::DecodeError;
use crate::packet::Packet;

struct Stream {
    reader: Box<dyn Read>,
    err: Option<Box<dyn Error>>,

    // TODO: Keep track of sequencers
    // sequencer: ????
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

#[cfg(test)]
mod tests {
    use super::*;
    use packed_struct::types::SizedInteger;
    use std::io::BufReader;

    #[test]
    fn test() {
        #[rustfmt::skip]
        let dat: &[u8] = &[
            // Primary/secondary header and a single byte of user data
            // byte 4 is sequence number 1 & 2
            0xd, 0x59, 0xc0, 0x01, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
            0xd, 0x59, 0xc0, 0x02, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];
        let reader = BufReader::new(dat);
        let stream = Stream {
            reader: Box::new(reader),
            err: None,
        };

        let packets: Vec<Packet> = stream
            .filter(|zult| zult.is_ok())
            .map(|zult| zult.unwrap())
            .collect();

        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].header.apid.to_primitive(), 1369);
        assert_eq!(packets[0].header.sequence_id.to_primitive(), 1);
        assert_eq!(packets[1].header.sequence_id.to_primitive(), 2);
    }
}
