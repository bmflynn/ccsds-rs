use std::{cmp::Ordering, collections::HashMap, fs::File, path::Path};

use anyhow::{bail, Context, Result};
use ccsds::spacepacket::{decode_packets, PrimaryHeader};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Key(u16, u16, u32);

pub fn diff(left_path: &Path, right_path: &Path, show_counts: bool) -> Result<()> {
    let mut apid_counts: HashMap<u16, (usize, usize)> = HashMap::default();

    let csum: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
    let mut left: Vec<Key> = decode_packets(File::open(left_path).context("opening left")?)
        .filter_map(Result::ok)
        .map(|p| {
            let count = apid_counts.entry(p.header.apid).or_default();
            *count = (count.0 + 1, count.1);
            Key(
                p.header.apid,
                p.header.sequence_id,
                csum.checksum(&p.data[PrimaryHeader::LEN..]),
            )
        })
        .collect();
    left.sort();
    let mut right: Vec<Key> = decode_packets(File::open(right_path).context("opening right")?)
        .filter_map(Result::ok)
        .map(|p| {
            let count = apid_counts.entry(p.header.apid).or_default();
            *count = (count.0, count.1 + 1);
            Key(
                p.header.apid,
                p.header.sequence_id,
                csum.checksum(&p.data[PrimaryHeader::LEN..]),
            )
        })
        .collect();
    right.sort();

    if left.is_empty() && right.is_empty() {
        bail!("no packets in left or right");
    }

    let mut apids: Vec<u16> = apid_counts.keys().cloned().collect();
    apids.sort();

    if show_counts {
        println!("Apid counts:");
        println!("APID  Left      Right     Diff");
        println!("====  ========  ========  ========");
        for apid in apids {
            let (left, right) = apid_counts.get(&apid).unwrap();
            if left != right {
                println!(
                    "{apid:4}  {left:8}  {right:8}  {:8}",
                    *left as i32 - *right as i32
                );
            }
        }
        println!();
    }

    println!();
    println!("left:  {}", left_path.to_string_lossy());
    println!("right: {}", right_path.to_string_lossy());
    println!();
    println!("Present in left, but not right         Present in right, but not left");
    println!("=====================================  ===================================");
    let print_left = |key: &Key| {
        println!(
            "[apid:{:4} seq:{:6} crc:{:10}]  [                                   ]",
            key.0, key.1, key.2
        )
    };
    let print_right = |key: &Key| {
        println!(
            "[                                   ]  [apid:{:4} seq:{:6} crc:{:10}]",
            key.0, key.1, key.2
        )
    };

    let mut differences = 0usize;
    let mut left = left.into_iter();
    let mut right = right.into_iter();
    let mut cached_left = left.next();
    let mut cached_right = right.next();
    loop {
        let Some(ref cur_left) = cached_left else {
            // no more left keys remaining, just print the rights and exit
            for key in right {
                differences += 1;
                print_right(&key);
            }
            break;
        };

        let Some(ref cur_right) = cached_right else {
            // no more right keys remaining, just print the lefts and exit
            for key in left {
                differences += 1;
                print_left(&key);
            }
            break;
        };

        match cur_left.cmp(cur_right) {
            Ordering::Less => {
                differences += 1;
                print_left(cur_left);
                cached_left = left.next();
            }
            Ordering::Greater => {
                differences += 1;
                print_right(cur_right);
                cached_right = right.next();
            }
            Ordering::Equal => {
                cached_left = left.next();
                cached_right = right.next();
            }
        }
    }

    if differences != 0 {
        bail!("{differences} packet differences");
    }
    Ok(())
}
