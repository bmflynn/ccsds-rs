use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{missing_packets, Apid, Packet, PrimaryHeader};
use crate::framing::Vcid;

/// Identifies a packet stream. The same APID may be used on more than one VCID, so a stream is
/// identified by `(vcid, apid)`. `vcid` is `None` when packets do not have a known VCID. See
/// [Packet::vcid].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PacketKey {
    pub vcid: Option<Vcid>,
    pub apid: Apid,
}

impl PacketKey {
    #[must_use]
    pub fn new(vcid: Option<Vcid>, apid: Apid) -> Self {
        PacketKey { vcid, apid }
    }
}

impl From<&Packet> for PacketKey {
    fn from(p: &Packet) -> Self {
        PacketKey::new(p.vcid, p.header.apid)
    }
}

/// serde cannot use a struct as a JSON map key, so maps keyed by [PacketKey] are serialized as a
/// sequence of `(key, value)` pairs sorted by key.
mod key_map {
    use std::collections::HashMap;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::PacketKey;

    pub fn serialize<S, V>(map: &HashMap<PacketKey, V>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        V: Serialize,
    {
        let mut items: Vec<(&PacketKey, &V)> = map.iter().collect();
        items.sort_by_key(|(k, _)| **k);
        items.serialize(s)
    }

    pub fn deserialize<'de, D, V>(d: D) -> Result<HashMap<PacketKey, V>, D::Error>
    where
        D: Deserializer<'de>,
        V: Deserialize<'de>,
    {
        Ok(Vec::<(PacketKey, V)>::deserialize(d)?.into_iter().collect())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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
    /// Per-stream stats, keyed by `(vcid, apid)`.
    #[serde(with = "key_map")]
    pub apids: HashMap<PacketKey, ApidSummary>,

    #[serde(with = "key_map")]
    seen_headers: HashMap<PacketKey, PrimaryHeader>,
}

impl Summary {
    pub fn add(&mut self, packet: &Packet) {
        self.count += 1;
        self.bytes += packet.data.len();

        let hdr = packet.header;
        let key = PacketKey::from(packet);
        let apid = self.apids.entry(key).or_default();
        apid.count += 1;
        apid.bytes += packet.data.len();

        if let Some(last_hdr) = self.seen_headers.get(&key) {
            let missing = missing_packets(hdr.sequence_id, last_hdr.sequence_id) as usize;
            apid.missing += missing;
            self.missing += missing;
        }
        self.seen_headers.insert(key, hdr);
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
        let key = PacketKey::new(None, 1369);
        assert_eq!(summary.apids[&key].count, 2);
        assert_eq!(summary.apids[&key].bytes, 30);
        assert_eq!(summary.apids[&key].missing, 0);
    }

    #[test]
    fn summary_same_apid_different_vcid() {
        #[rustfmt::skip]
        let dat: &[u8] = &[
            0xd, 0x59, 0xc0, 0x01, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
            0xd, 0x59, 0xc0, 0x02, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];
        // Same APID, but each packet on its own VCID with a gap in sequence ids between
        // them. Missing counts must not mix streams.
        let mut summary = Summary::default();
        let mut a = Packet::decode(&dat[0..15]).unwrap();
        a.vcid = Some(1);
        let mut b = Packet::decode(&dat[15..]).unwrap();
        b.vcid = Some(2);
        let mut c = Packet::decode(&dat[15..]).unwrap();
        c.vcid = Some(1);
        summary.add(&a);
        summary.add(&b);
        summary.add(&c);

        assert_eq!(summary.apids.len(), 2);
        assert_eq!(summary.apids[&PacketKey::new(Some(1), 1369)].count, 2);
        assert_eq!(summary.apids[&PacketKey::new(Some(2), 1369)].count, 1);
        assert_eq!(summary.missing, 0);

        // keyed maps must round trip through serde
        let json = serde_json::to_string(&summary).unwrap();
        let back: Summary = serde_json::from_str(&json).unwrap();
        assert_eq!(back.apids.len(), 2);
    }
}
