use ccsds::*;
use std::env;
use std::fs;
use std::io::Error as IoError;

#[test]
fn packet_iter() {
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let fpath = format!("{}/tests/fixtures/viirs_packets.dat", dir);
    let mut reader = fs::File::open(fpath).unwrap();
    let iter = PacketReaderIter::new(&mut reader);

    let packets: Vec<Result<Packet, IoError>> = iter.collect();

    assert_eq!(packets.len(), 100);
}

#[test]
fn group_iter() {
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let fpath = format!("{}/tests/fixtures/viirs_packets.dat", dir);
    let mut reader = fs::File::open(fpath).unwrap();
    let iter = PacketGroupIter::with_reader(&mut reader);

    let groups: Vec<Result<PacketGroup, IoError>> = iter.collect();

    assert_eq!(groups.len(), 7);

    // expected_counts is derived from edosl0util(0.16.0) collect_groups results
    let expected_counts = vec![1, 17, 17, 17, 17, 17, 14];
    for (idx, (group, expected_count)) in groups.iter().zip(expected_counts).enumerate() {
        let group = group.as_ref().unwrap();
        let count = group.packets.len();
        assert_eq!(
            count, expected_count,
            "Expected {} packets in group at index {}, got {}",
            expected_count, idx, count
        );
    }
}

#[test]
fn block_iter() {
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let fpath = format!("{}/tests/fixtures/snpp_7cadus_2vcids.dat", dir);
    let reader = fs::File::open(fpath).unwrap();

    let sync = Synchronizer::new(reader, &ASM[..].to_vec(), 1020);

    let iter = sync.into_iter();

    let mut count = 0;
    for zult in iter {
        zult.unwrap();
        count += 1;
    }
    assert_eq!(count, 7, "expected 7 total cadus")
}

#[test]
fn finds_first_asm() {
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let fpath = format!("{}/tests/fixtures/snpp_7cadus_2vcids.dat", dir);
    let reader = fs::File::open(fpath).unwrap();

    let mut sync = Synchronizer::new(reader, &ASM[..].to_vec(), 1020);
    let loc = sync.scan().unwrap();

    assert_eq!(loc.unwrap().offset, 5);
}
