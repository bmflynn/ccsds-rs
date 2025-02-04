use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::Path,
};

use anyhow::{bail, Context, Result};
use ccsds::spacepacket::{decode_packets, PrimaryHeader};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Key(u16, u16, u32);

pub fn diff(left: &Path, right: &Path, verbose: bool) -> Result<()> {
    let mut counts: HashMap<u16, (usize, usize)> = HashMap::default();

    let csum: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
    let left: HashSet<Key> = decode_packets(File::open(left).context("opening left")?)
        .filter_map(Result::ok)
        .map(|p| {
            let count = counts.entry(p.header.apid).or_default();
            *count = (count.0 + 1, count.1);
            Key(
                p.header.apid,
                p.header.sequence_id,
                csum.checksum(&p.data[PrimaryHeader::LEN..]),
            )
        })
        .collect();
    let right: HashSet<Key> = decode_packets(File::open(right).context("opening right")?)
        .filter_map(Result::ok)
        .map(|p| {
            let count = counts.entry(p.header.apid).or_default();
            *count = (count.0, count.1 + 1);
            Key(
                p.header.apid,
                p.header.sequence_id,
                csum.checksum(&p.data[PrimaryHeader::LEN..]),
            )
        })
        .collect();

    if left.is_empty() && right.is_empty() {
        bail!("no packets in left or right");
    }

    let mut apids: Vec<u16> = counts.keys().cloned().collect();
    apids.sort();

    let mut mismatch = false;
    for (left, right) in counts.values() {
        if left != right {
            mismatch = true;
        }
    }
    if !mismatch {
        if verbose {
            println!("Ok");
        }
        return Ok(());
    }

    println!("Apid counts:");
    println!("APID  Left      Right     Diff");
    println!("====  ========  ========  ========");
    for apid in apids {
        let (left, right) = counts.get(&apid).unwrap();
        if left != right {
            println!(
                "{apid:4}  {left:8}  {right:8}  {:8}",
                *left as i32 - *right as i32
            );
        }
    }

    let mut diff: Vec<&Key> = left.difference(&right).collect();
    diff.sort();
    if !diff.is_empty() {
        println!("\n{} packets in left, but not right", diff.len());
        if verbose {
            for key in diff {
                println!("apid:{:4} seq:{:6} crc:{:10}", key.0, key.1, key.2);
            }
        }
    }

    bail!("differences present");
}
