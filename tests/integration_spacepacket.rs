use ccsds::spacepacket::{Group, GroupIter, Packet, PacketIter};
use std::fs;
use std::io::Error;
use std::env;

#[test]
fn test_packet_iter() {
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let fpath = format!("{}/tests/fixtures/viirs_packets.dat", dir);
    let mut reader = fs::File::open(fpath).unwrap();
    let iter = PacketIter::new(&mut reader);

    let packets: Vec<Result<Packet, Error>> = iter.collect();

    assert_eq!(packets.len(), 100);
}

#[test]
fn test_group_iter() {
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let fpath = format!("{}/tests/fixtures/viirs_packets.dat", dir);
    let mut reader = fs::File::open(fpath).unwrap();
    let iter = GroupIter::new(&mut reader);

    let groups: Vec<Result<Group, Error>> = iter.collect();

    assert_eq!(groups.len(), 7);

    // expected_counts is derived from edosl0util(0.16.0) collect_groups results 
    let expected_counts = vec![1, 17, 17, 17, 17, 17, 14];
    for (idx, (group, expected_count)) in groups.iter().zip(expected_counts).enumerate() {
        let group = group.as_ref().unwrap();
        let count = group.packets.len();
        assert_eq!(count, expected_count, "Expected {} packets in group at index {}, got {}", expected_count, idx, count);
    }
}
