use std::str::FromStr;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    hash::Hash,
    io::{BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use hifitime::{Duration, Epoch};
use tracing::{debug, error, trace, warn};

use crate::spacepacket::{Apid, Error, PrimaryHeader};

use super::{collect_groups, decode_packets, TimecodeDecoder};

/// Merge, sort, and deduplicate multiple packet data files into a single file.
///
/// Packets are sorted and deduplicated by packet time, apid, and sequence id.
/// Therefore, all packets in each packet data file must either have a time that can
/// be decoded using `time_decoder` or be part of a packet group with a first packet
/// with a time that can be decoded by `time_decoder`.
///
/// Packets are all grouped when merging using [collect_groups] when merging. Any incomplete
/// groups, i.e., groups where [PacketGroup::complete](super::PacketGroup) returns `false`,
/// are dropped and not merged.
///
/// Additionally, any packet groups where a timecode cannot be successfully decode are dropped.
pub struct Merger {
    paths: Vec<PathBuf>,
    time_decoder: TimecodeDecoder,
    /// Maps an APID to its value used for ordering.
    order: HashMap<Apid, i32>,
    from: Option<u64>,
    to: Option<u64>,
    apids: Option<Vec<Apid>>,
}

impl Merger {
    pub fn new<S: AsRef<Path>>(paths: Vec<S>, decoder: TimecodeDecoder) -> Self {
        Self {
            paths: paths.iter().map(|s| s.as_ref().to_path_buf()).collect(),
            time_decoder: decoder,
            order: HashMap::default(),
            from: None,
            to: None,
            apids: None,
        }
    }

    /// Merged output will be sorted according the given order when multiple APIDs are available
    /// for a given time. APIDs appearing first in `order` will appear first in the output. If an
    /// APID is not in `order` its numerical APID value will be used to determine the order.
    pub fn with_apid_order(mut self, order: &[Apid]) -> Self {
        for (i, a) in order.iter().enumerate() {
            // order will be negative enough to not interfere with valid APIDs (0-2048) while still
            // supporting ordering as necessary.
            self.order.insert(*a, 4096 - i as i32);
        }
        self
    }

    /// Merged output will contain only data from and including `from` in microseconds.
    pub fn with_from(mut self, from: u64) -> Self {
        self.from = Some(from);
        self
    }

    /// Merged output will contain only data up to, but not including `to` in microseconds.
    pub fn with_to(mut self, to: u64) -> Self {
        self.to = Some(to);
        self
    }

    /// Merged output will only include the given APIDs.
    pub fn with_apids(mut self, apids: &[Apid]) -> Self {
        self.apids = Some(apids.to_vec());
        self
    }

    /// Perform the merge writing output to `writer`.
    pub fn merge<W: Write>(self, mut writer: W) -> Result<(), Error> {
        let to = epoch_or_default(self.to, 2200);
        let from = epoch_or_default(self.from, 1900);

        let apids: HashSet<Apid> = self.apids.unwrap_or_default().iter().copied().collect();
        let mut readers: HashMap<PathBuf, BufReader<File>> = HashMap::default();
        for path in self.paths {
            trace!("opening reader: {path:?}");
            readers.insert(path.clone(), BufReader::new(File::open(path)?));
        }

        let mut index: HashSet<Ptr> = HashSet::default();
        for (path, reader) in &mut readers {
            let packets = decode_packets(reader).filter_map(Result::ok);
            let pointers = collect_groups(packets)
                .filter_map(Result::ok)
                .filter_map(|g| {
                    if g.packets.is_empty() {
                        warn!("dropping group with no packets");
                        return None;
                    }
                    let first = &g.packets[0];
                    // If the first packet in the group is not a first or standalone packet the
                    // group is "corrupt"
                    if !(first.is_first() || first.is_standalone()) {
                        warn!(
                            header=?first.header,
                            packets = g.packets.len(),
                            "dropping bad group"
                        );
                        return None;
                    }

                    // Timecode comparisons
                    let Ok(epoch) = self.time_decoder.decode(first) else {
                        error!(header=?first.header, "timecode decode error; skipping");
                        return None;
                    };
                    if epoch < from {
                        debug!(?epoch, "dropping group before 'from'");
                        return None;
                    }
                    if epoch >= to {
                        debug!(?epoch, "dropping group after 'to'");
                        return None;
                    }
                    if !apids.is_empty() && !apids.contains(&first.header.apid) {
                        debug!(apid = first.header.apid, "dropping apid not in list");
                        return None;
                    }

                    // total size of all packets in group
                    let total_size = g
                        .packets
                        .iter()
                        .map(|p| PrimaryHeader::LEN + p.header.len_minus1 as usize + 1)
                        .sum();

                    Some(Ptr {
                        path: (*path).clone(),
                        offset: first.offset,
                        time: epoch,
                        apid: first.header.apid,
                        seqid: first.header.sequence_id,
                        size: total_size,
                        order: *self
                            .order
                            .get(&first.header.apid)
                            .unwrap_or(&(first.header.apid as i32)),
                    })
                })
                .collect::<HashSet<_>>();

            index = index.union(&pointers).cloned().collect();
        }

        let mut index: Vec<Ptr> = index.into_iter().collect();
        // Sort by time and apid, or the order index if set
        index.sort_by_key(|ptr| (ptr.time, ptr.order));

        for ptr in &index {
            // We know path is in readers
            let reader = readers.get_mut(&ptr.path).unwrap();
            trace!("seeking to pointer: {ptr:?}");
            reader.seek(SeekFrom::Start(ptr.offset as u64))?;

            let mut buf = vec![0u8; ptr.size];
            reader.read_exact(&mut buf)?;

            trace!("writing packet: {ptr:?}");
            writer.write_all(&buf)?;
        }

        Ok(())
    }
}

fn epoch_or_default(t: Option<u64>, year: u64) -> Epoch {
    t.map_or_else(
        || Epoch::from_str(&format!("{year}-01-01T00:00:00Z")).unwrap(),
        |micros| Epoch::from_utc_duration(Duration::compose(0, 0, 0, 0, 0, 0, micros, 0)),
    )
}

#[derive(Debug, Clone)]
struct Ptr {
    path: PathBuf,
    offset: usize,
    size: usize,

    // The following are considered for hashing purposes
    time: Epoch,
    apid: Apid,
    seqid: u16,

    // Sets the order packets are sorted in
    order: i32,
}

impl Hash for Ptr {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.apid.hash(state);
        self.time.hash(state);
        self.seqid.hash(state);
    }
}

impl PartialEq for Ptr {
    fn eq(&self, other: &Self) -> bool {
        self.apid == other.apid && self.time == other.time && self.seqid == other.seqid
    }
}

impl Eq for Ptr {}
