use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{missing_packets, Apid, Packet, PrimaryHeader};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ApidSummary {
    pub count: usize,
    pub bytes: usize,
    pub missing: usize,
}

/// Tracks stats on packet iteration.
///
/// # Example
/// ```
/// use std::io::Read;
/// use ccsds::spacepacket::{Packet, decode_packets, Summary};
/// let dat: &[u8] = &[0xd, 0x59, 0xc0, 0x01, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff];
///
/// let mut summary = Summary::default();
/// let packets: Vec<Packet> = decode_packets(dat)
///     .filter_map(Result::ok)
///     .inspect(|p| {
///         summary.add(p);
///     })
///     .collect();
/// ```
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub count: usize,
    pub bytes: usize,
    pub missing: usize,
    pub apids: HashMap<Apid, ApidSummary>,

    seen_headers: HashMap<Apid, PrimaryHeader>,
}

impl Summary {
    pub fn add(&mut self, packet: &Packet) {
        self.count += 1;
        self.bytes += packet.data.len();

        let hdr = packet.header;
        let apid = self.apids.entry(hdr.apid).or_default();
        apid.count += 1;
        apid.bytes += packet.data.len();

        if let Some(last_hdr) = self.seen_headers.get(&hdr.apid) {
            let missing = missing_packets(hdr.sequence_id, last_hdr.sequence_id) as usize;
            apid.missing += missing;
            self.missing += missing;
        }
        self.seen_headers.insert(hdr.apid, hdr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary() {
        #[rustfmt::skip]
        let dat: &[u8] = &[
            // Primary/secondary header and a single byte of user data
            // byte 4 is sequence number 1 & 2
            0xd, 0x59, 0xc0, 0x01, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
            0xd, 0x59, 0xc0, 0x02, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];

        // FIXME: Testing the summary should probably be a separate test
        let mut summary = Summary::default();
        let packet = Packet::decode(&dat[0..15]).unwrap();
        summary.add(&packet);
        let packet = Packet::decode(&dat[15..]).unwrap();
        summary.add(&packet);

        assert_eq!(summary.count, 2);
        assert_eq!(summary.bytes, 30);
        assert_eq!(summary.missing, 0);
        assert_eq!(summary.apids.len(), 1);
        assert_eq!(summary.apids[&1369].count, 2);
        assert_eq!(summary.apids[&1369].bytes, 30);
        assert_eq!(summary.apids[&1369].missing, 0);
    }
}
