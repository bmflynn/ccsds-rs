mod common;

use ccsds::timecode;
use std::fs::{self, File};
use std::result::Result;

use ccsds::spacepacket::{
    collect_groups, decode_packets, Merger, Packet, PacketGroup, TimecodeDecoder,
};

use common::fixture_path;

#[test]
fn packet_iter() {
    let fpath = fixture_path("viirs_packets.dat");
    let reader = fs::File::open(fpath).unwrap();
    let iter = decode_packets(reader);

    let packets: Vec<Result<Packet, ccsds::Error>> = iter.collect();

    assert_eq!(packets.len(), 100);
}

#[test]
fn group_iter() {
    let fpath = fixture_path("viirs_packets.dat");
    let reader = fs::File::open(fpath).unwrap();
    let packets = decode_packets(reader).map(Result::unwrap);
    let iter = collect_groups(packets);
    let groups: Vec<Result<PacketGroup, ccsds::Error>> = iter.collect();

    assert_eq!(groups.len(), 7);

    // expected_counts is derived from edosl0util(0.16.0) collect_groups results
    let expected_counts = vec![1, 17, 17, 17, 17, 17, 14];
    for (idx, (group, expected_count)) in groups.iter().zip(expected_counts).enumerate() {
        let group = group.as_ref().unwrap();
        let count = group.packets.len();
        assert_eq!(
            count, expected_count,
            "Expected {expected_count} packets in group at index {idx}, got {count}",
        );
    }
}

#[test]
fn merge_test() {
    let tmpdir = tempfile::tempdir().unwrap();
    let out_path = tmpdir.path().join("output.dat");
    let out_file = File::create(&out_path).unwrap();
    Merger::new(
        vec![
            fixture_path("viirs_merge1.dat"),
            fixture_path("viirs_merge2.dat"),
        ],
        TimecodeDecoder::new(timecode::Format::Cds {
            num_day: 2,
            num_submillis: 2,
        }),
    )
    .merge(out_file)
    .unwrap();

    // Get the merged files' packet groups, sorted
    let merged = File::open(&out_path).unwrap();
    let packets: Vec<Packet> = decode_packets(merged).map(Result::unwrap).collect();
    assert_eq!(
        packets.len(),
        235,
        "Expected 235 packets, got {}",
        packets.len()
    );

    let mut groups: Vec<PacketGroup> = collect_groups(packets.into_iter())
        .map(Result::unwrap)
        .collect();
    assert_eq!(groups.len(), 20, "expected 20 total groups");

    groups.sort_by(|a, b| a.apid.cmp(&b.apid));
    for (i, group) in groups.iter().take(7).enumerate() {
        assert_eq!(group.apid, 800, "group {i} has wrong apid");
        assert!(group.complete(), "group {i} should be complete");
        assert_eq!(group.packets.len(), 17, "group {i} has wrong len");
    }
    // NOTE: last 801 group is incomplete, so we skip it
    for (i, group) in groups.iter().skip(7).take(5).enumerate() {
        assert_eq!(group.apid, 801, "group {i} has wrong apid");
        for p in &group.packets {
            println!("{:?}", p.header);
        }
        assert!(group.complete(), "group {i} should be complete");
        assert_eq!(group.packets.len(), 17, "group {i} has wrong len");
    }
    for (i, group) in groups.iter().skip(14).enumerate() {
        assert_eq!(group.apid, 826, "group {i} has wrong apid");
        assert!(group.complete(), "group {i} should be complete");
        assert_eq!(group.packets.len(), 1, "group {i} has wrong len");
    }
}
