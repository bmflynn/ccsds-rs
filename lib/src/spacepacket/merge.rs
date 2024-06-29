use std::{
    collections::{HashMap, HashSet},
    fs::File,
    hash::Hash,
    io::{BufReader, Error as IOError, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use tracing::trace;

use crate::spacepacket::{Apid, PacketGroupIter, PacketReaderIter, PrimaryHeader};

use super::TimeDecoder;

/// Merge, sort, and deduplicate multiple packet data files into a single file.
///
/// Packets are sorted and deduplicated by packet time, apid, and sequence id.
/// Therefore, all packets in each packet data file must either have a time that can
/// be decoded using `time_decoder` or be part of a packet group with a first packet
/// with a time that can be decoded by `time_decoder`.
///
/// ``order`` will set the order in which APIDs are written to ``writer`` when there
/// are multiple APIDs for a single time.
///
/// ``from`` and ``to``, if specified, will resulting in dropping any packets not within
/// the specified time range in microseconds.
///
/// ## Errors
/// Any errors that occur while performing IO are propagated.
#[allow(clippy::missing_panics_doc, clippy::module_name_repetitions)]
pub fn merge_by_timecode<S, T, W>(
    paths: &[S],
    time_decoder: &T,
    mut writer: W,
    order: Option<Vec<Apid>>,
    from: Option<u64>,
    to: Option<u64>,
    apids: Option<&[Apid]>,
) -> std::io::Result<()>
where
    S: AsRef<Path>,
    T: TimeDecoder,
    W: Write,
{
    let apids: HashSet<Apid> = apids.unwrap_or_default().iter().copied().collect();
    let mut readers: HashMap<PathBuf, BufReader<File>> = HashMap::default();
    for path in paths {
        let path = path.as_ref().to_path_buf();
        trace!("opening reader: {path:?}");
        readers.insert(path.clone(), BufReader::new(File::open(path)?));
    }
    let order: HashMap<Apid, Apid> = order
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, a)| (a, Apid::try_from(i).unwrap()))
        .collect();

    let mut index: HashSet<Ptr> = HashSet::default();
    for (path, reader) in &mut readers {
        let packets = PacketReaderIter::new(reader).filter_map(Result::ok);
        let pointers = PacketGroupIter::with_packets(packets)
            .filter_map(Result::ok)
            .filter_map(|g| {
                if g.packets.is_empty()
                    || !(g.packets[0].is_first() || g.packets[0].is_standalone())
                {
                    // Drop incomplete packet groups
                    return None;
                }
                let first = &g.packets[0];
                let usecs = time_decoder.decode_time(first).unwrap_or_else(|| {
                    panic!(
                        "failed to decode timecode from {first}: {:?}",
                        &first.data[..14]
                    )
                });

                // enforce time range, inclusice on the from, exclusive on to
                if from.is_some() && usecs < from.unwrap() {
                    return None;
                }
                if to.is_some() && usecs >= to.unwrap() {
                    return None;
                }
                if !apids.is_empty() && !apids.contains(&first.header.apid) {
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
                    time: usecs,
                    apid: first.header.apid,
                    seqid: first.header.sequence_id,
                    size: total_size,
                    order: *order.get(&first.header.apid).unwrap_or(&first.header.apid),
                })
            })
            .collect::<HashSet<_>>();

        index = index.union(&pointers).cloned().collect();
    }

    let mut index: Vec<Ptr> = index.into_iter().collect();
    index.sort_by_key(|ptr| (ptr.time, ptr.order));

    for ptr in &index {
        // We know path is in readers
        let reader = readers.get_mut(&ptr.path).unwrap();
        trace!("seeing to pointer: {ptr:?}");
        reader.seek(SeekFrom::Start(ptr.offset as u64))?;

        let mut buf = vec![0u8; ptr.size];
        if let Err(err) = reader.read_exact(&mut buf) {
            let msg = format!("Reading {ptr:?}: {err}");
            return Err(IOError::new(std::io::ErrorKind::Other, msg));
        }
        trace!("writing packet: {ptr:?}");
        writer.write_all(&buf)?;
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct Ptr {
    path: PathBuf,
    offset: usize,
    size: usize,

    // The following are considered for hashing purposes
    time: u64,
    apid: Apid,
    seqid: u16,

    // Sets the order packets are sorted in
    order: Apid,
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
