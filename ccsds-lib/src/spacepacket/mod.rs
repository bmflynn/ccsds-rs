mod merge;
mod summary;
mod timecode;

#[cfg(feature = "python")]
use pyo3::{prelude::*, types::PyBytes};

use std::fmt::Display;
use std::io::Read;

use serde::{Deserialize, Serialize};

pub use crate::prelude::*;
pub use merge::*;
pub use summary::*;
pub use timecode::*;

pub type Apid = u16;

/// Packet represents a single CCSDS space packet and its associated data.
///
/// This packet contains the primary header data as well as the user data,
/// which may or may not container a secondary header. See the header's
/// `has_secondary_header` flag.
///
/// # Example
/// Create a packet from the minimum number of bytes.
/// ```
/// use ccsds::spacepacket::{Packet, PrimaryHeader};
///
/// let dat: &[u8] = &[
///     // primary header bytes
///     0xd, 0x59, 0xd2, 0xab, 0x0, 07,
///     // Cds timecode bytes in secondary header (not decoded here)
///     0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
///     // minimum 1 byte of user data
///     0xff
/// ];
/// let packet = Packet::decode(dat).unwrap();
/// ```
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "python", pyclass(frozen))]
pub struct Packet {
    /// All packets have a primary header
    pub header: PrimaryHeader,
    /// All packet bytes, including header and user data
    pub data: Vec<u8>,

    pub(crate) offset: usize,
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

#[cfg_attr(feature = "python", pymethods)]
impl Packet {
    const MAX_LEN: usize = 65535;

    #[cfg(feature = "python")]
    #[getter]
    fn header(&self) -> PrimaryHeader {
        self.header
    }

    #[cfg(feature = "python")]
    #[new]
    fn py_new(buf: &[u8]) -> PyResult<Self> {
        //let buf = buf.as_bytes(py);
        Ok(Packet::decode(buf)?)
    }

    /// All packet data
    #[cfg(feature = "python")]
    #[getter]
    fn data<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new_bound(py, &self.data)
    }

    /// User data, i.e., no primary header data
    #[cfg(feature = "python")]
    #[getter]
    fn user_data<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new_bound(py, &self.data[PrimaryHeader::LEN..])
    }

    #[cfg(feature = "python")]
    fn __str__(&self) -> String {
        format!("{self}")
    }

    #[must_use]
    pub fn is_first(&self) -> bool {
        self.header.sequence_flags == PrimaryHeader::SEQ_FIRST
    }

    #[must_use]
    pub fn is_last(&self) -> bool {
        self.header.sequence_flags == PrimaryHeader::SEQ_LAST
    }

    #[must_use]
    pub fn is_cont(&self) -> bool {
        self.header.sequence_flags == PrimaryHeader::SEQ_CONTINUATION
    }

    #[must_use]
    pub fn is_standalone(&self) -> bool {
        self.header.sequence_flags == PrimaryHeader::SEQ_UNSEGMENTED
    }
}

impl Packet {
    /// Read a single [Packet].
    ///
    /// # Errors:
    /// [Error::NotEnoughData] if `buf` does not contain enough data for packet header and the
    /// length described by that header.
    pub fn decode(buf: &[u8]) -> Result<Packet> {
        if buf.len() < PrimaryHeader::LEN {
            return Err(Error::NotEnoughData {
                actual: buf.len(),
                minimum: PrimaryHeader::LEN,
            });
        }
        let ph = PrimaryHeader::decode(&buf[..PrimaryHeader::LEN])?;
        let data_len = ph.len_minus1 as usize + 1;
        let total_len = PrimaryHeader::LEN + data_len;
        if buf.len() < total_len {
            return Err(Error::NotEnoughData {
                actual: buf.len(),
                minimum: total_len,
            });
        }
        Ok(Packet {
            header: ph,
            data: buf[..total_len].to_vec(),
            offset: 0,
        })
    }
}

impl Packet {
    pub fn read<R>(file: &mut R) -> Result<Packet>
    where
        R: Read + Send,
    {
        let mut buf = vec![0u8; Packet::MAX_LEN];
        file.read_exact(&mut buf[..PrimaryHeader::LEN])?;

        let ph = PrimaryHeader::decode(&buf[..PrimaryHeader::LEN])?;
        let data_len = ph.len_minus1 as usize + 1;
        let total_len = PrimaryHeader::LEN + data_len;
        file.read_exact(&mut buf[PrimaryHeader::LEN..total_len])?;

        Ok(Packet {
            header: ph,
            data: buf[..total_len].to_vec(),
            offset: 0,
        })
    }
}

/// CCSDS Primary Header
///
/// The primary header format is common to all CCSDS space packets.
///
#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
#[cfg_attr(feature = "python", pyclass(frozen))]
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

#[cfg_attr(feature = "python", pymethods)]
impl PrimaryHeader {
    #[cfg(feature = "python")]
    #[getter]
    fn version(&self) -> u8 {
        self.version
    }
    #[cfg(feature = "python")]
    #[getter]
    fn type_flag(&self) -> u8 {
        self.type_flag
    }
    #[cfg(feature = "python")]
    #[getter]
    fn has_secondary_header(&self) -> bool {
        self.has_secondary_header
    }
    #[cfg(feature = "python")]
    #[getter]
    fn apid(&self) -> Apid {
        self.apid
    }
    #[cfg(feature = "python")]
    #[getter]
    fn sequence_flags(&self) -> u8 {
        self.sequence_flags
    }
    #[cfg(feature = "python")]
    #[getter]
    fn sequence_id(&self) -> u16 {
        self.sequence_id
    }
    #[cfg(feature = "python")]
    #[getter]
    fn len_minus1(&self) -> u16 {
        self.len_minus1
    }

    #[cfg(feature = "python")]
    fn __str__(&self) -> String {
        format!("{self:?}")
    }
}

impl PrimaryHeader {
    /// Size of a ``PrimaryHeader``
    pub const LEN: usize = 6;
    /// Maximum supported sequence id value
    pub const SEQ_MAX: u16 = 16383;
    /// Packet is the first packet in a packet group
    pub const SEQ_FIRST: u8 = 1;
    /// Packet is a part of a packet group, but not first and not last
    pub const SEQ_CONTINUATION: u8 = 0;
    /// Packet is the last packet in a packet group
    pub const SEQ_LAST: u8 = 2;
    /// Packet is not part of a packet group, i.e., standalone.
    pub const SEQ_UNSEGMENTED: u8 = 3;

    /// Decode from bytes. Returns `None` if there are not enough bytes to construct the
    /// header.
    pub fn decode(buf: &[u8]) -> Result<Self> {
        if buf.len() < Self::LEN {
            return Err(Error::NotEnoughData {
                actual: buf.len(),
                minimum: Self::LEN,
            });
        }
        let d1 = u16::from_be_bytes([buf[0], buf[1]]);
        let d2 = u16::from_be_bytes([buf[2], buf[3]]);
        let d3 = u16::from_be_bytes([buf[4], buf[5]]);

        Ok(PrimaryHeader {
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

/// Packet data representing a CCSDS packet group according to the packet
/// sequencing value in primary header.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "python", pyclass(frozen))]
pub struct PacketGroup {
    pub apid: Apid,
    pub packets: Vec<Packet>,
}

#[cfg_attr(feature = "python", pymethods)]
impl PacketGroup {
    #[cfg(feature = "python")]
    #[getter]
    fn apid(&self) -> Apid {
        self.apid
    }

    #[cfg(feature = "python")]
    #[getter]
    fn packets(&self) -> Vec<Packet> {
        self.packets.clone()
    }

    #[cfg(feature = "python")]
    fn __str__(&self) -> String {
        format!(
            "PacketGroup {{apid={} packets[len={}]}}",
            self.apid,
            self.packets.len()
        )
    }

    /// Return true if this packet group is complete.
    ///
    /// Valid means at least 1 packet and all the packets for a complete group with no missing
    /// packets.
    #[must_use]
    pub fn complete(&self) -> bool {
        if self.packets.is_empty() {
            false
        } else if self.packets.len() == 1 {
            self.packets[0].is_standalone()
        } else {
            self.packets[0].is_first()
                && self.packets.last().unwrap().is_last()
                && !self.have_missing()
        }
    }

    #[must_use]
    pub fn have_missing(&self) -> bool {
        for (a, b) in self.packets.iter().zip(self.packets[1..].iter()) {
            if missing_packets(b.header.sequence_id, a.header.sequence_id) > 0 {
                return true;
            }
        }
        false
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

/// Return an iterator providing [Packet] data read from a byte synchronized ungrouped
/// packet stream.
///
/// For packet streams that may contain packets that utilize packet grouping see
/// ``decode_packet_groups``.
///
/// # Examples
/// ```
/// use ccsds::spacepacket::decode_packets;
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
/// decode_packets(r).for_each(|zult| {
///     let packet = zult.unwrap();
///     assert_eq!(packet.header.apid, 1369);
/// });
/// ```
pub fn decode_packets<R>(reader: R) -> impl Iterator<Item = Result<Packet>> + Send
where
    R: Read + Send,
{
    PacketReaderIter::new(reader)
}

/// Return an [Iterator] that groups read packets into [PacketGroup]s.
///
/// This is necessary for packet streams containing APIDs that utilize packet grouping sequence
/// flags values [SEQ_FIRST](PrimaryHeader), [SEQ_CONTINUATION](PrimaryHeader), and
/// [SEQ_LAST](PrimaryHeader). It can also be used for
/// non-grouped APIDs ([SEQ_UNSEGMENTED](PrimaryHeader)), however, it is not necessary in such
/// cases and will result in each group containing a single packet.
/// See [sequence_flags](PrimaryHeader).
///
/// # Examples
///
/// Reading packets from data file of space packets (level-0) would look something
/// like this:
/// ```
/// use ccsds::spacepacket::{Packet, collect_groups};
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
/// let packets = vec![Packet::decode(dat).unwrap()];
/// collect_groups(packets.into_iter()).for_each(|zult| {
///     let packet = zult.unwrap();
///     assert_eq!(packet.apid, 1369);
/// });
/// ```
pub fn collect_groups<I>(packets: I) -> impl Iterator<Item = Result<PacketGroup>> + Send
where
    I: Iterator<Item = Packet> + Send,
{
    PacketGroupIter::with_packets(packets)
}

struct PacketReaderIter<R>
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
    type Item = Result<Packet>;

    fn next(&mut self) -> Option<Self::Item> {
        match Packet::read(&mut self.reader) {
            Ok(mut p) => {
                p.offset = self.offset;
                self.offset += PrimaryHeader::LEN + p.header.len_minus1 as usize + 1;
                Some(Ok(p))
            }
            Err(err) => {
                if let Error::Io(ref ioerr) = err {
                    if ioerr.kind() == std::io::ErrorKind::UnexpectedEof {
                        return None;
                    }
                }
                Some(Err(err))
            }
        }
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
    type Item = Result<PacketGroup>;

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

#[cfg(test)]
mod tests {
    use summary::Summary;

    use super::*;

    #[test]
    fn test_decode_packet() {
        let dat: [u8; 15] = [
            // Primary/secondary header and a single byte of user data
            0xd, 0x59, 0xd2, 0xab, 0x0, 0x8, 0x52, 0xc0, 0x0, 0x0, 0x0, 0xa7, 0x0, 0xdb, 0xff,
        ];
        let packet = Packet::decode(&dat).unwrap();

        assert_eq!(packet.header.version, 0);
    }

    #[test]
    fn test_decode_header() {
        let dat: [u8; 6] = [
            // bytes from a SNPP CrIS packet
            0xd, 0x59, 0xd2, 0xab, 0xa, 0x8f,
        ];
        let ph = PrimaryHeader::decode(&dat).unwrap();

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

        // FIXME: Testing the summary should probably be a separate test
        let mut summary = Summary::default();
        let packets: Vec<Packet> = decode_packets(dat)
            .map(|z| z.unwrap())
            .inspect(|p| {
                summary.add(p);
            })
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
